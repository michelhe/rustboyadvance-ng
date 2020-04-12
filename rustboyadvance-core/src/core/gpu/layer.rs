use num::FromPrimitive;
use std::cmp::Ordering;

use super::*;

#[repr(u8)]
#[derive(Primitive, Debug, Ord, Eq, PartialOrd, PartialEq, Clone, Copy)]
pub enum RenderLayerKind {
    // These match BlendFlags
    Backdrop = 0b0010_0000,
    Background3 = 0b0000_1000,
    Background2 = 0b0000_0100,
    Background1 = 0b0000_0010,
    Background0 = 0b0000_0001,
    Objects = 0b0001_0000,
}

impl RenderLayerKind {
    pub fn get_blend_flag(self) -> BlendFlags {
        BlendFlags::from_bits(self as u8).unwrap()
    }
}

#[derive(Debug, PartialEq, Eq)]
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
            pixel,
            priority,
        }
    }

    pub fn backdrop(pixel: Rgb15) -> RenderLayer {
        RenderLayer {
            kind: RenderLayerKind::Backdrop,
            pixel,
            priority: 4,
        }
    }

    pub(super) fn is_object(&self) -> bool {
        self.kind == RenderLayerKind::Objects
    }
}

impl PartialOrd<RenderLayer> for RenderLayer {
    fn partial_cmp(&self, other: &RenderLayer) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RenderLayer {
    fn cmp(&self, other: &RenderLayer) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.kind.cmp(&other.kind).reverse())
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
        layers.sort();
        assert_eq!(RenderLayer::background(3, pixel, 0), layers[0]);
    }
}
