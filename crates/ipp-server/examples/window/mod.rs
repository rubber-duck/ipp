//! Platform window boundary for the GLES development Host. All calls stay on
//! the Host's main thread; the EGL surface borrows the native layer until drop.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::Window;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::Window;

/// Fixed native-shim record; the development channel encodes fields explicitly.
/// Kinds: move, down, up, wheel, key, text, blur, resize, close (1 through 9).
/// Points are normalized, wheel deltas are logical pixels; resize uses physical
/// width/height in x/y and device scale in dx. Key codes are platform-neutral.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct WindowEvent {
    pub kind: u32,
    pub code: u32,
    pub modifiers: u32,
    pub length: u32,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub text: [u8; 128],
}

impl Default for WindowEvent {
    fn default() -> Self {
        Self {
            kind: 0,
            code: 0,
            modifiers: 0,
            length: 0,
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
            text: [0; 128],
        }
    }
}

impl WindowEvent {
    pub(crate) fn encode(&self, bytes: &mut Vec<u8>) {
        for value in [self.kind, self.code, self.modifiers, self.length] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [self.x, self.y, self.dx, self.dy] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.text);
    }
}
