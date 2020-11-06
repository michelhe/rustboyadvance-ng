use super::*;
use regs::RegMosaic;

impl RegMosaic {
    fn is_enabled_for_bg(&self) -> bool {
        (self.bg_hsize() != 0) || (self.bg_vsize() != 0)
    }
}

impl Gpu {
    fn mosaic_bg(&mut self) {
        // let hsize = (self.mosaic.bg_hsize() + 1) as usize;
        // let vsize = (self.mosaic.bg_vsize() + 1) as usize;

        // for bg in 0..4 {
        //     if self.dispcnt.enable_bg[bg] && self.backgrounds[bg].mosaic {
        //         let y = self.vcount as usize;
        //         if y % vsize == 0 {
        //             self.backgrounds[bg].mosaic_first_row = self.backgrounds[bg].line.clone();
        //         }
        //         for x in 0..DISPLAY_WIDTH {
        //             let color = self.backgrounds[bg].mosaic_first_row[(x / hsize) * hsize];
        //             self.backgrounds[bg].line[x] = color;
        //         }
        //     }
        // }
    }

    pub fn mosaic_sfx(&mut self) {
        if self.mosaic.is_enabled_for_bg() {
            self.mosaic_bg();
        }
        // TODO obj mosaic
    }
}
