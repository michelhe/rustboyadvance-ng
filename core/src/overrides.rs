use std::collections::HashMap;
use std::convert::TryFrom;

use yaml_rust::YamlLoader;

use super::cartridge::BackupType;

#[derive(Debug)]
pub struct GameOverride {
    force_rtc: bool,
    save_type: Option<BackupType>,
}

impl GameOverride {
    pub fn force_rtc(&self) -> bool {
        self.force_rtc
    }
    pub fn save_type(&self) -> Option<BackupType> {
        self.save_type
    }
}

lazy_static! {
    static ref GAME_OVERRIDES: HashMap<String, GameOverride> = {
        let mut m = HashMap::new();

        let docs = YamlLoader::load_from_str(include_str!("../overrides.yaml"))
            .expect("failed to load overrides file");

        let doc = &docs[0];
        let games = doc.as_vec().unwrap();

        for game in games {
            let game_code = String::from(game["code"].as_str().unwrap());
            let force_rtc = game["rtc"].as_bool().unwrap_or(false);
            let save_type = if let Some(save_type) = game["save_type"].as_str() {
                match BackupType::try_from(save_type) {
                    Ok(x) => Some(x),
                    _ => panic!("{}: invalid save type {:#}", game_code, save_type),
                }
            } else {
                None
            };

            let game_overrride = GameOverride {
                force_rtc,
                save_type,
            };
            m.insert(game_code, game_overrride);
        }

        m
    };
}

pub fn get_game_overrides(game_code: &str) -> Option<&GameOverride> {
    GAME_OVERRIDES.get(game_code)
}
