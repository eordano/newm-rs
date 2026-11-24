//! Qt key codes, as KGlobalAccel sends them over D-Bus (`QKeyCombination::toCombined()`: a
//! `Qt::Key` or-ed with `Qt::KeyboardModifier` bits), to and from xkb keysyms plus modifiers,
//! which is how newm's key bindings match keys: the keysym of the key's first level (`s` for
//! Shift+S) and the modifiers held.

/// Modifier bits, the same as niri-config's `Modifiers`.
pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 1 << 1;
pub const ALT: u8 = 1 << 2;
pub const SUPER: u8 = 1 << 3;

pub const QT_SHIFT: i32 = 0x0200_0000;
pub const QT_CTRL: i32 = 0x0400_0000;
pub const QT_ALT: i32 = 0x0800_0000;
pub const QT_META: i32 = 0x1000_0000;
const QT_KEYPAD: i32 = 0x2000_0000;
const QT_KEY: i32 = 0x01FF_FFFF;

const QT_F1: i32 = 0x0100_0030;
const QT_F35: i32 = 0x0100_0052;
const XKB_F1: u32 = 0xffbe;
const XKB_F35: u32 = 0xffe0;
const QT_ENTER: i32 = 0x0100_0005;
const XKB_KP_ENTER: u32 = 0xff8d;
const XKB_KP_0: u32 = 0xffb0;
const XKB_KP_9: u32 = 0xffb9;

/// Qt keys outside Latin-1 and the F keys, with their keysyms.
const SPECIAL: &[(i32, u32)] = &[
    (0x0100_0000, 0xff1b),      // Escape
    (0x0100_0001, 0xff09),      // Tab
    (0x0100_0002, 0xfe20),      // Backtab = ISO_Left_Tab
    (0x0100_0003, 0xff08),      // Backspace
    (0x0100_0004, 0xff0d),      // Return
    (0x0100_0006, 0xff63),      // Insert
    (0x0100_0007, 0xffff),      // Delete
    (0x0100_0008, 0xff13),      // Pause
    (0x0100_0009, 0xff61),      // Print
    (0x0100_000a, 0xff15),      // SysReq
    (0x0100_000b, 0xff0b),      // Clear
    (0x0100_0010, 0xff50),      // Home
    (0x0100_0011, 0xff57),      // End
    (0x0100_0012, 0xff51),      // Left
    (0x0100_0013, 0xff52),      // Up
    (0x0100_0014, 0xff53),      // Right
    (0x0100_0015, 0xff54),      // Down
    (0x0100_0016, 0xff55),      // PageUp
    (0x0100_0017, 0xff56),      // PageDown
    (0x0100_0020, 0xffe1),      // Shift = Shift_L
    (0x0100_0021, 0xffe3),      // Control = Control_L
    (0x0100_0022, 0xffeb),      // Meta = Super_L
    (0x0100_0023, 0xffe9),      // Alt = Alt_L
    (0x0100_0024, 0xffe5),      // CapsLock
    (0x0100_0025, 0xff7f),      // NumLock
    (0x0100_0026, 0xff14),      // ScrollLock
    (0x0100_0053, 0xffeb),      // Super_L
    (0x0100_0054, 0xffec),      // Super_R
    (0x0100_0055, 0xff67),      // Menu
    (0x0100_0058, 0xff6a),      // Help
    (0x0100_0061, 0x1008_ff26), // Back = XF86Back
    (0x0100_0062, 0x1008_ff27), // Forward
    (0x0100_0063, 0x1008_ff28), // Stop
    (0x0100_0064, 0x1008_ff29), // Refresh
    (0x0100_0070, 0x1008_ff11), // VolumeDown = XF86AudioLowerVolume
    (0x0100_0071, 0x1008_ff12), // VolumeMute = XF86AudioMute
    (0x0100_0072, 0x1008_ff13), // VolumeUp = XF86AudioRaiseVolume
    (0x0100_0080, 0x1008_ff14), // MediaPlay = XF86AudioPlay
    (0x0100_0081, 0x1008_ff15), // MediaStop
    (0x0100_0082, 0x1008_ff16), // MediaPrevious = XF86AudioPrev
    (0x0100_0083, 0x1008_ff17), // MediaNext
    (0x0100_0084, 0x1008_ff1c), // MediaRecord
    (0x0100_0085, 0x1008_ff31), // MediaPause
    (0x0100_0090, 0x1008_ff18), // HomePage
    (0x0100_0091, 0x1008_ff30), // Favorites
    (0x0100_0092, 0x1008_ff1b), // Search
    (0x0100_00a0, 0x1008_ff19), // LaunchMail = XF86Mail
    (0x0100_00a1, 0x1008_ff32), // LaunchMedia = XF86AudioMedia
    (0x0100_00a2, 0x1008_ff33), // Launch0 = XF86MyComputer
    (0x0100_00b2, 0x1008_ff02), // MonBrightnessUp
    (0x0100_00b3, 0x1008_ff03), // MonBrightnessDown
    (0x0100_00b4, 0x1008_ff04), // KeyboardLightOnOff
    (0x0100_00b5, 0x1008_ff05), // KeyboardBrightnessUp
    (0x0100_00b6, 0x1008_ff06), // KeyboardBrightnessDown
    (0x0100_00b7, 0x1008_ff2a), // PowerOff
    (0x0100_00b8, 0x1008_ff2b), // WakeUp
    (0x0100_00b9, 0x1008_ff2c), // Eject
    (0x0100_00ba, 0x1008_ff2d), // ScreenSaver
    (0x0100_00cb, 0x1008_ff1d), // Calculator
    (0x0100_0110, 0x1008_ffa9), // TouchpadToggle
    (0x0100_0113, 0x1008_ffb2), // MicMute = XF86AudioMicMute
];

/// Keypad keysyms that are not digits, with the Qt key that carries the keypad bit for them.
const KEYPAD: &[(u32, i32)] = &[
    (0xffaa, b'*' as i32),
    (0xffab, b'+' as i32),
    (0xffac, b',' as i32),
    (0xffad, b'-' as i32),
    (0xffae, b'.' as i32),
    (0xffaf, b'/' as i32),
    (XKB_KP_ENTER, QT_ENTER),
];

/// The keysym and modifiers of one Qt key combination; `None` for 0 or a key newm cannot bind.
pub fn from_qt(combined: i32) -> Option<(u32, u8)> {
    let key = combined & QT_KEY;
    if key == 0 {
        return None;
    }
    let mut mods = 0;
    for (qt, bit) in [
        (QT_SHIFT, SHIFT),
        (QT_CTRL, CTRL),
        (QT_ALT, ALT),
        (QT_META, SUPER),
    ] {
        if combined & qt != 0 {
            mods |= bit;
        }
    }

    if combined & QT_KEYPAD != 0 {
        if (i32::from(b'0')..=i32::from(b'9')).contains(&key) {
            return Some((XKB_KP_0 + (key - i32::from(b'0')) as u32, mods));
        }
        if let Some(&(keysym, _)) = KEYPAD.iter().find(|(_, qt)| *qt == key) {
            return Some((keysym, mods));
        }
    }

    let keysym = match key {
        0x41..=0x5a => key as u32 + 0x20,
        0x20..=0x7e => key as u32,
        0xc0..=0xde if key != 0xd7 => key as u32 + 0x20,
        0xa0..=0xff => key as u32,
        QT_F1..=QT_F35 => XKB_F1 + (key - QT_F1) as u32,
        QT_ENTER => XKB_KP_ENTER,
        _ => SPECIAL.iter().find(|(qt, _)| *qt == key)?.1,
    };
    Some((keysym, mods))
}

/// The Qt key combination of a keysym and modifiers; `None` for a keysym Qt has no key for here.
pub fn to_qt(keysym: u32, mods: u8) -> Option<i32> {
    let key = match keysym {
        0x61..=0x7a => keysym as i32 - 0x20,
        0x20..=0x7e => keysym as i32,
        0xe0..=0xfe if keysym != 0xf7 => keysym as i32 - 0x20,
        0xa0..=0xff => keysym as i32,
        XKB_KP_0..=XKB_KP_9 => (i32::from(b'0') + (keysym - XKB_KP_0) as i32) | QT_KEYPAD,
        XKB_F1..=XKB_F35 => QT_F1 + (keysym - XKB_F1) as i32,
        _ => {
            if let Some(&(_, qt)) = KEYPAD.iter().find(|(k, _)| *k == keysym) {
                qt | QT_KEYPAD
            } else {
                SPECIAL.iter().find(|(_, k)| *k == keysym)?.0
            }
        }
    };
    let mut combined = key;
    for (bit, qt) in [
        (SHIFT, QT_SHIFT),
        (CTRL, QT_CTRL),
        (ALT, QT_ALT),
        (SUPER, QT_META),
    ] {
        if mods & bit != 0 {
            combined |= qt;
        }
    }
    Some(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_S: i32 = 0x53;
    const KEY_PRINT: i32 = 0x0100_0009;

    #[test]
    fn letters_are_upper_case_in_qt_and_first_level_keysyms_in_newm() {
        assert_eq!(
            from_qt(KEY_S | QT_META | QT_SHIFT),
            Some((0x73, SUPER | SHIFT))
        );
        assert_eq!(to_qt(0x73, SUPER | SHIFT), Some(KEY_S | QT_META | QT_SHIFT));
        assert_eq!(to_qt(0x53, SHIFT), Some(KEY_S | QT_SHIFT));
    }

    #[test]
    fn spectacles_print_and_media_keys_round_trip() {
        assert_eq!(from_qt(KEY_PRINT), Some((0xff61, 0)));
        assert_eq!(from_qt(KEY_PRINT | QT_SHIFT), Some((0xff61, SHIFT)));
        for &(qt, keysym) in SPECIAL {
            let (sym, mods) = from_qt(qt | QT_CTRL).unwrap();
            assert_eq!((sym, mods), (keysym, CTRL));
            // Qt's Meta and Super_L name one keysym: the way back may answer either.
            let back = to_qt(keysym, CTRL).unwrap();
            assert_eq!(from_qt(back), Some((keysym, CTRL)), "{qt:#x}");
        }
    }

    #[test]
    fn f_keys_digits_and_punctuation() {
        assert_eq!(from_qt(QT_F1 + 11), Some((XKB_F1 + 11, 0)));
        assert_eq!(to_qt(XKB_F35, ALT), Some(QT_F35 | QT_ALT));
        assert_eq!(from_qt(i32::from(b'1') | QT_META), Some((0x31, SUPER)));
        assert_eq!(from_qt(i32::from(b'/') | QT_CTRL), Some((0x2f, CTRL)));
        assert_eq!(to_qt(0x20, SUPER), Some(0x20 | QT_META));
    }

    #[test]
    fn keypad_keys_carry_qts_keypad_bit() {
        assert_eq!(
            from_qt(i32::from(b'5') | QT_KEYPAD),
            Some((XKB_KP_0 + 5, 0))
        );
        assert_eq!(to_qt(XKB_KP_0 + 5, 0), Some(i32::from(b'5') | QT_KEYPAD));
        assert_eq!(from_qt(QT_ENTER | QT_KEYPAD), Some((XKB_KP_ENTER, 0)));
        assert_eq!(
            to_qt(0xffab, CTRL),
            Some(i32::from(b'+') | QT_KEYPAD | QT_CTRL)
        );
    }

    #[test]
    fn latin_1_letters_fold_like_ascii() {
        // Ä in Qt, ä as the keysym; × and ÷ are no letters.
        assert_eq!(from_qt(0xc4), Some((0xe4, 0)));
        assert_eq!(to_qt(0xe4, 0), Some(0xc4));
        assert_eq!(from_qt(0xd7), Some((0xd7, 0)));
        assert_eq!(to_qt(0xf7, 0), Some(0xf7));
    }

    #[test]
    fn nothing_and_unknown_keys_bind_nothing() {
        assert_eq!(from_qt(0), None);
        assert_eq!(from_qt(QT_META), None);
        assert_eq!(from_qt(0x0100_ffff), None);
        assert_eq!(to_qt(0x1008_fe01, 0), None);
    }
}
