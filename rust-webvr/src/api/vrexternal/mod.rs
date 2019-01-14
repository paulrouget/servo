mod display;
mod service;

use libc;
use std::os::raw::c_void;
use {VRService, VRServiceCreator};

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(dead_code)]
mod mozgfx {
    include!(concat!(env!("OUT_DIR"), "/moz_external_vr.rs"));
}

#[derive(Clone)]
pub struct VRExternalShmemPtr(*mut mozgfx::VRExternalShmem);

unsafe impl Send for VRExternalShmemPtr {}
unsafe impl Sync for VRExternalShmemPtr {}

impl VRExternalShmemPtr {
    pub fn new(raw: *mut c_void) -> VRExternalShmemPtr {
        VRExternalShmemPtr(raw as *mut mozgfx::VRExternalShmem)
    }

    pub fn pull_system(&self, condition: &Fn(&mozgfx::VRSystemState) -> bool) -> mozgfx::VRSystemState {
        unsafe {
            let mutex = &mut (*self.0).systemMutex as *mut _ as *mut libc::pthread_mutex_t;
            let cond = &mut (*self.0).systemCond as *mut _ as *mut libc::pthread_cond_t;
            let r = libc::pthread_mutex_lock(mutex);
            assert_eq!(r, 0);
            while condition(&(*self.0).state) {
                let r = libc::pthread_cond_wait(cond, mutex);
                assert_eq!(r, 0);
            }
            let state = (*self.0).state.clone();
            let r = libc::pthread_mutex_unlock(mutex);
            assert_eq!(r, 0);
            state
        }
    }

    pub fn pull_browser(&self) -> mozgfx::VRBrowserState {
        unsafe {
            let mutex = &mut (*self.0).browserMutex as *mut _ as *mut libc::pthread_mutex_t;
            let r = libc::pthread_mutex_lock(mutex);
            assert_eq!(r, 0);
            let state = (*self.0).servoState.clone();
            let r = libc::pthread_mutex_unlock(mutex);
            assert_eq!(r, 0);
            state
        }
    }

    pub fn push_browser(&mut self, state: mozgfx::VRBrowserState) {
        unsafe {
            let mutex = &mut (*self.0).browserMutex as *mut _ as *mut libc::pthread_mutex_t;
            let cond = &mut (*self.0).browserCond as *mut _ as *mut libc::pthread_cond_t;
            let r = libc::pthread_mutex_lock(mutex);
            assert_eq!(r, 0);
            (*self.0).servoState = state;
            let r = libc::pthread_cond_signal(cond);
            assert_eq!(r, 0);
            let r = libc::pthread_mutex_unlock(mutex);
            assert_eq!(r, 0);
        }
    }
}

pub struct VRExternalServiceCreator(VRExternalShmemPtr);

impl VRExternalServiceCreator {
    pub fn new(ptr: VRExternalShmemPtr) -> Box<VRServiceCreator> {
        Box::new(VRExternalServiceCreator(ptr))
    }
}

impl VRServiceCreator for VRExternalServiceCreator {
    fn new_service(&self) -> Box<VRService> {
        Box::new(service::VRExternalService::new(self.0.clone()))
    }
}
