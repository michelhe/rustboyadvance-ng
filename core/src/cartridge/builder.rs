use std::path::{Path, PathBuf};

use memmem::{Searcher, TwoWaySearcher};
use num::FromPrimitive;

use super::super::overrides;
use super::super::{GBAError, GBAResult};
use super::backup::eeprom::*;
use super::backup::flash::*;
use super::backup::{BackupFile, BackupType};
use super::gpio::Gpio;
use super::header;
use super::BackupMedia;
use super::Cartridge;

use super::loader::{load_from_bytes, load_from_file, LoadRom};

#[derive(Debug)]
#[allow(dead_code)]
pub enum GpioDeviceType {
    Rtc,
    SolarSensor,
    Gyro,
    None,
}

#[derive(Debug)]
pub struct GamepakBuilder {
    path: Option<PathBuf>,
    bytes: Option<Box<[u8]>>,
    save_path: Option<PathBuf>,
    save_type: BackupType,
    gpio_device: GpioDeviceType,
    create_backup_file: bool,
}

impl GamepakBuilder {
    pub fn new() -> GamepakBuilder {
        GamepakBuilder {
            save_type: BackupType::AutoDetect,
            path: None,
            save_path: None,
            bytes: None,
            gpio_device: GpioDeviceType::None,
            create_backup_file: true,
        }
    }

    pub fn take_buffer(mut self, bytes: Box<[u8]>) -> Self {
        self.bytes = Some(bytes);
        self
    }

    pub fn buffer(mut self, bytes: &[u8]) -> Self {
        self.bytes = Some(bytes.into());
        self
    }

    pub fn file(mut self, path: &Path) -> Self {
        self.path = Some(path.to_path_buf());
        self
    }

    pub fn save_path(mut self, path: &Path) -> Self {
        self.save_path = Some(path.to_path_buf());
        self
    }

    pub fn save_type(mut self, save_type: BackupType) -> Self {
        self.save_type = save_type;
        self
    }

    pub fn with_sram(mut self) -> Self {
        self.save_type = BackupType::Sram;
        self
    }

    pub fn with_flash128k(mut self) -> Self {
        self.save_type = BackupType::Flash1M;
        self
    }

    pub fn with_flash64k(mut self) -> Self {
        self.save_type = BackupType::Flash512;
        self
    }

    pub fn with_eeprom(mut self) -> Self {
        self.save_type = BackupType::Eeprom;
        self
    }

    pub fn without_backup_to_file(mut self) -> Self {
        self.create_backup_file = false;
        self
    }

    pub fn with_rtc(mut self) -> Self {
        self.gpio_device = GpioDeviceType::Rtc;
        self
    }

    pub fn build(mut self) -> GBAResult<Cartridge> {
        let (bytes, symbols) = if let Some(bytes) = self.bytes {
            match load_from_bytes(bytes.to_vec())? {
                #[cfg(feature = "elf_support")]
                LoadRom::Elf { data, symbols } => Ok((data, Some(symbols))),
                LoadRom::Raw(data) => Ok((data, None)),
            }
        } else if let Some(path) = &self.path {
            match load_from_file(&path)? {
                #[cfg(feature = "elf_support")]
                LoadRom::Elf { data, symbols } => Ok((data, Some(symbols))),
                LoadRom::Raw(data) => Ok((data, None)),
            }
        } else {
            Err(GBAError::CartridgeLoadError(
                "either provide file() or buffer()".to_string(),
            ))
        }?;

        let header = header::parse(&bytes)?;
        info!("Loaded ROM: {:?}", header);

        if !self.create_backup_file {
            self.save_path = None;
        } else if self.save_path.is_none() {
            if let Some(path) = &self.path {
                self.save_path = Some(path.with_extension(BACKUP_FILE_EXT));
            } else {
                warn!("can't create save file as no save path was provided")
            }
        }

        let mut save_type = self.save_type;
        let mut gpio_device = self.gpio_device;

        if let Some(overrides) = overrides::get_game_overrides(&header.game_code) {
            info!(
                "Found game overrides for {}: {:#?}",
                header.game_code, overrides
            );
            if let Some(override_save_type) = overrides.save_type() {
                if override_save_type != save_type && save_type != BackupType::AutoDetect {
                    warn!(
                        "Forced save type {:?} takes priority of {:?}",
                        save_type, override_save_type
                    );
                }
                save_type = override_save_type;
            }

            if overrides.force_rtc() {
                match gpio_device {
                    GpioDeviceType::None => gpio_device = GpioDeviceType::Rtc,
                    GpioDeviceType::Rtc => {}
                    _ => {
                        warn!(
                            "Can't use RTC due to forced gpio device type {:?}",
                            gpio_device
                        );
                    }
                }
            }
        }

        if save_type == BackupType::AutoDetect {
            if let Some(detected) = detect_backup_type(&bytes) {
                info!("Detected Backup: {:?}", detected);
                save_type = detected;
            } else {
                warn!("could not detect backup save type");
            }
        }

        let backup = create_backup(save_type, self.save_path);

        let gpio = match gpio_device {
            GpioDeviceType::None => None,
            GpioDeviceType::Rtc => {
                info!("Emulating RTC!");
                Some(Gpio::new_rtc())
            }
            _ => unimplemented!("Gpio device {:?} not implemented", gpio_device),
        };

        let size = bytes.len();
        Ok(Cartridge {
            header: header,
            gpio: gpio,
            bytes: bytes.into_boxed_slice(),
            size: size,
            backup: backup,
            symbols: symbols,
        })
    }
}

const BACKUP_FILE_EXT: &'static str = "sav";
fn create_backup(backup_type: BackupType, rom_path: Option<PathBuf>) -> BackupMedia {
    let backup_path = if let Some(rom_path) = rom_path {
        Some(rom_path.with_extension(BACKUP_FILE_EXT))
    } else {
        None
    };
    match backup_type {
        BackupType::Flash | BackupType::Flash512 => {
            BackupMedia::Flash(Flash::new(backup_path, FlashSize::Flash64k))
        }
        BackupType::Flash1M => BackupMedia::Flash(Flash::new(backup_path, FlashSize::Flash128k)),
        BackupType::Sram => BackupMedia::Sram(BackupFile::new(0x8000, backup_path)),
        BackupType::Eeprom => BackupMedia::Eeprom(EepromController::new(backup_path)),
        BackupType::AutoDetect => BackupMedia::Undetected,
    }
}

fn detect_backup_type(bytes: &[u8]) -> Option<BackupType> {
    const ID_STRINGS: &'static [&'static str] =
        &["EEPROM", "SRAM", "FLASH_", "FLASH512_", "FLASH1M_"];

    for i in 0..5 {
        let search = TwoWaySearcher::new(ID_STRINGS[i].as_bytes());
        match search.search_in(bytes) {
            Some(_) => return Some(BackupType::from_u8(i as u8).unwrap()),
            _ => {}
        }
    }
    None
}
