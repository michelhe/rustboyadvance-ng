use num::FromPrimitive;

use super::*;

#[derive(Primitive, Debug, Ord, Eq, PartialOrd, PartialEq, Clone, Copy)]
pub enum RenderLayerKind {
    Backdrop = 0b00100000,
    Background3 = 0b00001000,
    Background2 = 0b00000100,
    Background1 = 0b00000010,
    Background0 = 0b00000001,
    Objects = 0b00010000,
}

impl RenderLayerKind {
    pub fn get_blend_flag(&self) -> BlendFlags {
        match self {
            RenderLayerKind::Background0 => BlendFlags::BG0,
            RenderLayerKind::Background1 => BlendFlags::BG1,
            RenderLayerKind::Background2 => BlendFlags::BG2,
            RenderLayerKind::Background3 => BlendFlags::BG3,
            RenderLayerKind::Objects => BlendFlags::OBJ,
            RenderLayerKind::Backdrop => BlendFlags::BACKDROP,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct RenderLayer {
    pub kind: RenderLayerKind,
    pub priority: u16,
    pub pixel: Rgb15,
}

impl RenderLayer {
    pub fn background(bg: usize, pixel: Rgb15, priority: u16) -> RenderLayer {
        RenderLayer {
            kind: RenderLayerKind::from_usize(1 << bg).unwrap(),
            pixel: pixel,
            priority: priority,
        }
    }

    pub fn objects(pixel: Rgb15, priority: u16) -> RenderLayer {
        RenderLayer {
            kind: RenderLayerKind::Objects,
            pixel: pixel,
            priority: priority,
        }
    }

    pub fn backdrop(pixel: Rgb15) -> RenderLayer {
        RenderLayer {
            kind: RenderLayerKind::Backdrop,
            pixel: pixel,
            priority: 4,
        }
    }

    pub(super) fn is_object(&self) -> bool {
        self.kind == RenderLayerKind::Objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrayvec::ArrayVec;

    #[test]
    fn test_layer_sort_order() {
        let mut layers = ArrayVec::<[_; 7]>::new();

        let backdrop = Rgb15(0xaaaa);
        let pixel = Rgb15::WHITE;
        layers.push(RenderLayer::background(0, pixel, 3));
        layers.push(RenderLayer::backdrop(backdrop));
        layers.push(RenderLayer::background(1, pixel, 2));
        layers.push(RenderLayer::background(3, pixel, 0));
        layers.push(RenderLayer::background(2, pixel, 2));
        layers.push(RenderLayer::backdrop(backdrop));
        layers.push(RenderLayer::objects(pixel, 1));
        layers.sort_by_key(|k| (k.priority, k.kind));
        assert_eq!(RenderLayer::background(3, pixel, 0), layers[0]);
    }
}
