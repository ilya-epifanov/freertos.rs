use prelude::v1::*;
use base::*;
use task::*;
use shim::*;
use mutex::*;
use queue::*;
use units::*;

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

/// A FreeRTOS software timer.
///
/// Note that all operations on a timer are processed by a FreeRTOS internal task
/// that receives messages in a queue. Every operation has an associated waiting time
/// for that queue to get unblocked.
pub struct Timer {
    handle: FreeRtosTimerHandle,
    detached: bool
}

/// Helper builder for a new software timer.
pub struct TimerBuilder {
    name: String,
    period: Duration,
    auto_reload: bool
}

impl TimerBuilder {
    /// Set the name of the timer.
    pub fn set_name(&mut self, name: &str) -> &mut Self {
        self.name = name.into();
        self
    }

    /// Set the period of the timer.
    pub fn set_period(&mut self, period: Duration) -> &mut Self {
        self.period = period;
        self
    }

    /// Should the timer be automatically reloaded?
    pub fn set_auto_reload(&mut self, auto_reload: bool) -> &mut Self {
        self.auto_reload = auto_reload;
        self
    }
    
    /// Try to create the new timer.
    ///
    /// Note that the newly created timer must be started.
    pub fn create<F>(&self, callback: F) -> Result<Timer, FreeRtosError>
        where F: Fn(Timer) -> (),
              F: Send + 'static
    {
        Timer::spawn(self.name.as_str(), self.period, self.auto_reload, callback)
    }
}



impl Timer {
    /// Create a new timer builder.
    pub fn new() -> TimerBuilder {
        TimerBuilder {
            name: "timer".into(),
            period: Duration::ms(1000),
            auto_reload: true
        }
    }

    unsafe fn spawn_inner<'a>(name: &str,
                              period: Duration,
                              auto_reload: bool,
                              callback: Box<Fn(Timer) + Send + 'a>,)
                              -> Result<Timer, FreeRtosError> {
        let f = Box::new(callback);
        let param_ptr = &*f as *const _ as *mut _;

        let (success, timer_handle) = {
            let name = name.as_bytes();
            let name_len = name.len();
            let mut timer_handle = mem::zeroed::<CVoid>();

            let ret = freertos_rs_timer_create(name.as_ptr(),
                                               name_len as u8,
                                               period.to_ticks(),
                                               if auto_reload { 1 } else { 0 },
                                               param_ptr,
                                               timer_callback);

            ((ret as usize) != 0, ret)
        };

        if success {
            mem::forget(f);
        } else {
            return Err(FreeRtosError::OutOfMemory);
        }

        extern "C" fn timer_callback(handle: FreeRtosTimerHandle) -> () {
            unsafe {                
                {
                    let mut timer = Timer {
                        handle: handle,
                        detached: true
                    };
                    if let Ok(callback_ptr) = timer.get_id() {
                        let b = Box::from_raw(callback_ptr as *mut Box<Fn(Timer)>);
                        b(timer);
                        Box::into_raw(b);
                    }
                }
            }
        }

        Ok(Timer { 
            handle: timer_handle as *const _,
            detached: false
        })
    }


    fn spawn<F>(name: &str,
                period: Duration,
                auto_reload: bool,
                callback: F)
                -> Result<Timer, FreeRtosError>
        where F: Fn(Timer) -> (),
              F: Send + 'static
    {
        unsafe {
            Timer::spawn_inner(name, period, auto_reload, Box::new(callback))
        }
    }

    /// Start the timer.
    pub fn start(&self, block_time: Duration) -> Result<(), FreeRtosError> {
        unsafe {
            if freertos_rs_timer_start(self.handle, block_time.to_ticks()) == 0 {
                Ok(())
            } else {
                Err(FreeRtosError::Timeout)
            }
        }
    }

    /// Stop the timer.
    pub fn stop(&self, block_time: Duration) -> Result<(), FreeRtosError> {
        unsafe {
            if freertos_rs_timer_stop(self.handle, block_time.to_ticks()) == 0 {
                Ok(())
            } else {
                Err(FreeRtosError::Timeout)
            }
        }
    }

    /// Change the period of the timer.
    pub fn change_period(&self, block_time: Duration, new_period: Duration) -> Result<(), FreeRtosError> {
        unsafe {
            if freertos_rs_timer_change_period(self.handle, block_time.to_ticks(), new_period.to_ticks()) == 0 {
                Ok(())
            } else {
                Err(FreeRtosError::Timeout)
            }
        }
    }

    /// Detach this timer from Rust's memory management. The timer will still be active and
    /// will consume the memory.
    ///
    /// Can be used for timers that will never be changed and don't need to stay in scope.
    pub unsafe fn detach(mut self) {
        self.detached = true;
    }

    fn get_id(&self) -> Result<FreeRtosVoidPtr, FreeRtosError> {
        unsafe {
            Ok(freertos_rs_timer_get_id(self.handle))
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.detached == true { return; }

        unsafe {
            if let Ok(callback_ptr) = self.get_id() {
                // free the memory
                Box::from_raw(callback_ptr as *mut Box<Fn(Timer)>);
            }
            
            // todo: configurable timeout?
            freertos_rs_timer_delete(self.handle, Duration::ms(1000).to_ticks());
        }
    }
}
