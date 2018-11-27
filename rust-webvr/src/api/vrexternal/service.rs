use super::display::VRExternalDisplay;
use super::VRExternalShmemPtr;
use {VRDisplayPtr, VREvent, VRGamepadPtr, VRService};

pub struct VRExternalService(VRExternalShmemPtr);

impl VRService for VRExternalService {
    fn initialize(&mut self) -> Result<(), String> {
        debug!("PAUL: VRExternalService::initialize");
        Ok(())
    }

    fn fetch_displays(&mut self) -> Result<Vec<VRDisplayPtr>, String> {
        Ok(vec![VRExternalDisplay::new(self.0.clone())])
    }

    fn fetch_gamepads(&mut self) -> Result<Vec<VRGamepadPtr>, String> {
        Ok(Vec::new())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn poll_events(&self) -> Vec<VREvent> {
        Vec::new()
    }
}

impl VRExternalService {
    pub fn new(ptr: VRExternalShmemPtr) -> VRExternalService {
        VRExternalService(ptr)
    }
}
