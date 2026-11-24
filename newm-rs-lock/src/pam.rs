//! Password check through libpam: `pam_authenticate` then `pam_acct_mgmt` for the current user.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

use crate::auth::Authenticator;
use crate::secret::wipe;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_NEW_AUTHTOK_REQD: c_int = 12;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_MAX_NUM_MSG: c_int = 32;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = unsafe extern "C" fn(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConvFn>,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        pamh: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut PamHandle, status: c_int) -> c_int;
}

pub struct Pam {
    service: CString,
    user: CString,
}

impl Pam {
    pub fn new(service: &str) -> Result<Pam, String> {
        let service = CString::new(service).map_err(|e| e.to_string())?;
        let pw = unsafe { libc::getpwuid(libc::getuid()) };
        if pw.is_null() {
            return Err(format!("no passwd entry for uid {}", unsafe {
                libc::getuid()
            }));
        }
        let user = unsafe { CStr::from_ptr((*pw).pw_name) }.to_owned();
        Ok(Pam { service, user })
    }
}

impl Authenticator for Pam {
    fn authenticate(&self, password: &[u8]) -> bool {
        if password.contains(&0) {
            return false;
        }
        let conv = PamConv {
            conv: Some(converse),
            appdata_ptr: &password as *const &[u8] as *mut c_void,
        };
        let mut handle = ptr::null_mut();
        let rc = unsafe {
            pam_start(
                self.service.as_ptr(),
                self.user.as_ptr(),
                &conv,
                &mut handle,
            )
        };
        if rc != PAM_SUCCESS {
            eprintln!("newm-rs-lock: pam_start failed ({rc})");
            if !handle.is_null() {
                unsafe { pam_end(handle, rc) };
            }
            return false;
        }
        let mut rc = unsafe { pam_authenticate(handle, 0) };
        if rc == PAM_SUCCESS {
            rc = unsafe { pam_acct_mgmt(handle, 0) };
            if rc == PAM_NEW_AUTHTOK_REQD {
                rc = PAM_SUCCESS;
            }
        }
        unsafe { pam_end(handle, rc) };
        rc == PAM_SUCCESS
    }
}

unsafe fn free_replies(replies: *mut PamResponse, count: usize) {
    for i in 0..count {
        let resp = unsafe { (*replies.add(i)).resp };
        if !resp.is_null() {
            let len = unsafe { libc::strlen(resp) };
            wipe(unsafe { std::slice::from_raw_parts_mut(resp.cast::<u8>(), len) });
            unsafe { libc::free(resp.cast()) };
        }
    }
    unsafe { libc::free(replies.cast()) };
}

unsafe extern "C" fn converse(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if !(1..=PAM_MAX_NUM_MSG).contains(&num_msg) || msg.is_null() || resp.is_null() {
        return PAM_CONV_ERR;
    }
    let count = num_msg as usize;
    let password: &[u8] = unsafe { *appdata.cast::<&[u8]>() };
    let replies: *mut PamResponse = unsafe { libc::calloc(count, size_of::<PamResponse>()) }.cast();
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..count {
        let style = unsafe { (**msg.add(i)).msg_style };
        match style {
            PAM_PROMPT_ECHO_OFF => {
                let copy: *mut u8 = unsafe { libc::calloc(password.len() + 1, 1) }.cast();
                if copy.is_null() {
                    unsafe { free_replies(replies, count) };
                    return PAM_BUF_ERR;
                }
                unsafe {
                    ptr::copy_nonoverlapping(password.as_ptr(), copy, password.len());
                    (*replies.add(i)).resp = copy.cast();
                }
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {}
            _ => {
                unsafe { free_replies(replies, count) };
                return PAM_CONV_ERR;
            }
        }
    }
    unsafe { *resp = replies };
    PAM_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(styles: &[c_int], password: &[u8]) -> (c_int, Vec<Option<Vec<u8>>>) {
        let messages: Vec<PamMessage> = styles
            .iter()
            .map(|&s| PamMessage {
                msg_style: s,
                msg: c"prompt".as_ptr(),
            })
            .collect();
        let mut pointers: Vec<*const PamMessage> = messages.iter().map(|m| m as *const _).collect();
        let mut out: *mut PamResponse = ptr::null_mut();
        let appdata = &password as *const &[u8] as *mut c_void;
        let rc = unsafe {
            converse(
                styles.len() as c_int,
                pointers.as_mut_ptr(),
                &mut out,
                appdata,
            )
        };
        if rc != PAM_SUCCESS {
            return (rc, Vec::new());
        }
        let answers = (0..styles.len())
            .map(|i| {
                let r = unsafe { (*out.add(i)).resp };
                (!r.is_null()).then(|| unsafe { CStr::from_ptr(r) }.to_bytes().to_vec())
            })
            .collect();
        unsafe { free_replies(out, styles.len()) };
        (rc, answers)
    }

    #[test]
    fn answers_the_hidden_prompt_with_the_password() {
        let (rc, answers) = ask(&[PAM_PROMPT_ECHO_OFF], b"hunter2");
        assert_eq!(rc, PAM_SUCCESS);
        assert_eq!(answers, vec![Some(b"hunter2".to_vec())]);
    }

    #[test]
    fn info_and_error_messages_get_no_answer() {
        let (rc, answers) = ask(&[PAM_TEXT_INFO, PAM_PROMPT_ECHO_OFF, PAM_ERROR_MSG], b"pw");
        assert_eq!(rc, PAM_SUCCESS);
        assert_eq!(answers, vec![None, Some(b"pw".to_vec()), None]);
    }

    #[test]
    fn visible_prompts_are_refused() {
        let (rc, _) = ask(&[PAM_PROMPT_ECHO_OFF, 2], b"pw");
        assert_eq!(rc, PAM_CONV_ERR);
    }

    #[test]
    fn message_count_is_bounded() {
        assert_eq!(ask(&[], b"pw").0, PAM_CONV_ERR);
        assert_eq!(ask(&[PAM_TEXT_INFO; 33], b"pw").0, PAM_CONV_ERR);
    }

    #[test]
    fn current_user_is_found() {
        let pam = Pam::new("newm-lock").unwrap();
        assert!(!pam.user.as_bytes().is_empty());
        assert_eq!(pam.service.as_bytes(), b"newm-lock");
    }

    #[test]
    fn interior_nul_is_rejected_without_calling_pam() {
        let pam = Pam::new("newm-lock-test-service-that-does-not-exist").unwrap();
        assert!(!pam.authenticate(b"a\0b"));
    }
}
