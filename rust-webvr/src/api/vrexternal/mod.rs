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

pub struct SystemMem<'a>(&'a mut mozgfx::VRExternalShmem);

impl<'a> SystemMem<'a> {
    pub fn new(mem: &'a mut mozgfx::VRExternalShmem) -> SystemMem {
        unsafe {
            let mutex = (&mut mem.systemMutex) as *mut _ as *mut libc::pthread_mutex_t;
            let r = libc::pthread_mutex_lock(mutex);
            assert_eq!(r, 0);
        }
        SystemMem(mem)
    }
    pub fn as_ref(&self) -> &mozgfx::VRSystemState {
        &self.0.state
    }
}

impl<'a> Drop for SystemMem<'a> {
    fn drop(&mut self) {
        let mutex = &mut self.0.systemMutex as *mut _ as *mut libc::pthread_mutex_t;
        unsafe {
            let r = libc::pthread_mutex_unlock(mutex);
            assert_eq!(r, 0);
        }
    }
}

pub struct BrowserMem<'a>(&'a mut mozgfx::VRExternalShmem);

impl<'a> BrowserMem<'a> {
    pub fn new(mem: &'a mut mozgfx::VRExternalShmem) -> BrowserMem {
        unsafe {
            let mutex = (&mut mem.browserMutex) as *mut _ as *mut libc::pthread_mutex_t;
            let r = libc::pthread_mutex_lock(mutex);
            assert_eq!(r, 0);
        }
        BrowserMem(mem)
    }
    pub fn as_ref(&self) -> &mozgfx::VRBrowserState {
        &self.0.servoState
    }
    pub fn as_mut(&mut self) -> &mut mozgfx::VRBrowserState {
        &mut self.0.servoState
    }
}

impl<'a> Drop for BrowserMem<'a> {
    fn drop(&mut self) {
        let mutex = &mut self.0.browserMutex as *mut _ as *mut libc::pthread_mutex_t;
        let cond = &mut self.0.browserCond as *mut _ as *mut libc::pthread_cond_t;
        unsafe {
            let r = libc::pthread_cond_signal(cond);
            assert_eq!(r, 0);
            let r = libc::pthread_mutex_unlock(mutex);
            assert_eq!(r, 0);
        }
    }
}

impl VRExternalShmemPtr {
    pub fn new(raw: *mut c_void) -> VRExternalShmemPtr {
        VRExternalShmemPtr(raw as *mut mozgfx::VRExternalShmem)
    }

    pub fn system(&self) -> SystemMem {
        SystemMem::new(unsafe { &mut *self.0 })
    }

    pub fn browser(&self) -> BrowserMem {
        BrowserMem::new(unsafe { &mut *self.0 })
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
