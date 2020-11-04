use serde::{Deserialize, Serialize};

use super::consts::*;
use super::WindowFlags;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Window {
    pub left: u8,
    pub right: u8,
    pub top: u8,
    pub bottom: u8,
    pub flags: WindowFlags,
}

impl Window {
    pub fn inside(&self, x: usize, y: usize) -> bool {
        let left = self.left();
        let right = self.right();
        self.contains_y(y) && (x >= left && x < right)
    }

    #[inline]
    pub fn left(&self) -> usize {
        self.left as usize
    }

    #[inline]
    pub fn right(&self) -> usize {
        let left = self.left as usize;
        let mut right = self.right as usize;
        if right > DISPLAY_WIDTH || right < left {
            right = DISPLAY_WIDTH;
        }
        right
    }

    #[inline]
    pub fn top(&self) -> usize {
        self.top as usize
    }

    #[inline]
    pub fn bottom(&self) -> usize {
        let top = self.top as usize;
        let mut bottom = self.bottom as usize;
        if bottom > DISPLAY_HEIGHT || bottom < top {
            bottom = DISPLAY_HEIGHT;
        }
        bottom
    }

    #[inline]
    pub fn contains_y(&self, y: usize) -> bool {
        let top = self.top();
        let bottom = self.bottom();
        y >= top && y < bottom
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum WindowType {
    Win0,
    Win1,
    WinObj,
    WinOut,
    WinNone,
}

#[derive(Debug)]
pub struct WindowInfo {
    pub typ: WindowType,
    pub flags: WindowFlags,
}

impl WindowInfo {
    pub fn new(typ: WindowType, flags: WindowFlags) -> WindowInfo {
        WindowInfo { typ, flags }
    }

    #[inline]
    pub fn is_none(&self) -> bool {
        self.typ == WindowType::WinNone
    }
}
