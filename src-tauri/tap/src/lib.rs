//! Nightmoth taskbar helper, loaded into Explorer through XAML Diagnostics (the
//! mechanism TranslucentTB uses). It finds the taskbar's background layers
//! ("BackgroundFill" / "BackgroundStroke") and fades them out, so only the icons show.
//!
//! It touches nothing else, and follows a switch Nightmoth sets in the registry:
//! HKCU\Software\Nightmoth\TaskbarClear = 1 (hide) / 0 (show again).
//!
//! Safety: every COM call checks its result; the opacity is read back and only written
//! when the read looks right; panics abort instead of unwinding into Explorer. If
//! Explorer still restarts right after the helper loads, Nightmoth stops loading it.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Duration;

use windows::core::{implement, interface, IInspectable, IUnknown, IUnknown_Vtbl, Interface, Ref, BSTR, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_NOINTERFACE, E_POINTER, S_FALSE};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::UI::Core::{CoreDispatcher, CoreDispatcherPriority, DispatchedHandler};

/// {6B3A2B1E-8F4C-4E3B-9A51-4E49474854D0} — must match the app.
pub const CLSID_TAP: GUID = GUID::from_u128(0x6b3a2b1e_8f4c_4e3b_9a51_4e49474854d0);

const NAMES: &[&str] = &["BackgroundFill", "BackgroundStroke"];

// ------------------------------------------------------------------ XAML diagnostics (xamlOM.h)

#[repr(C)]
pub struct ParentChildRelation {
    pub parent: u64,
    pub child: u64,
    pub child_index: u32,
}

#[repr(C)]
pub struct SourceInfo {
    pub file_name: std::mem::ManuallyDrop<BSTR>,
    pub line: u32,
    pub column: u32,
    pub char_pos: u32,
    pub hash: std::mem::ManuallyDrop<BSTR>,
}

#[repr(C)]
pub struct VisualElement {
    pub handle: u64,
    pub src: SourceInfo,
    pub type_name: std::mem::ManuallyDrop<BSTR>,
    pub name: std::mem::ManuallyDrop<BSTR>,
    pub num_children: u32,
}

#[interface("18C9E2B6-3F43-4116-9F2B-FF935D7770D2")]
unsafe trait IXamlDiagnostics: IUnknown {
    fn GetDispatcher(&self, out: *mut *mut c_void) -> HRESULT;
    fn GetUiLayer(&self, out: *mut *mut c_void) -> HRESULT;
    fn GetApplication(&self, out: *mut *mut c_void) -> HRESULT;
    fn GetIInspectableFromHandle(&self, handle: u64, out: *mut *mut c_void) -> HRESULT;
}

#[interface("A593B11A-D17F-48BB-8F66-83910731C8A5")]
unsafe trait IVisualTreeService: IUnknown {
    fn AdviseVisualTreeChange(&self, callback: *mut c_void) -> HRESULT;
}

// structs bigger than 8 bytes are passed by pointer in the x64 calling convention
#[interface("AA7A8931-80E4-4FEC-8F3B-553F87B4966E")]
unsafe trait IVisualTreeServiceCallback: IUnknown {
    fn OnVisualTreeChange(&self, relation: *const ParentChildRelation, element: *const VisualElement, mutation: i32) -> HRESULT;
}

#[interface("BAD9EB88-AE77-4397-B948-5FA2DB0A19EA")]
unsafe trait IVisualTreeServiceCallback2: IVisualTreeServiceCallback {
    fn OnElementStateChanged(&self, element: u64, state: i32, context: PCWSTR) -> HRESULT;
}

/// Windows.UI.Xaml.IUIElement — only the first five methods (after IInspectable's three).
#[interface("676D0BE9-B65C-41C6-BA40-58CF87F201C1")]
unsafe trait IUIElement: IUnknown {
    fn GetIids(&self, count: *mut u32, iids: *mut *mut GUID) -> HRESULT;
    fn GetRuntimeClassName(&self, name: *mut *mut c_void) -> HRESULT;
    fn GetTrustLevel(&self, level: *mut i32) -> HRESULT;
    fn get_DesiredSize(&self, out: *mut [f32; 2]) -> HRESULT;
    fn get_AllowDrop(&self, out: *mut u8) -> HRESULT;
    fn put_AllowDrop(&self, v: u8) -> HRESULT;
    fn get_Opacity(&self, out: *mut f64) -> HRESULT;
    fn put_Opacity(&self, v: f64) -> HRESULT;
}

// ------------------------------------------------------------------ state

struct Shared {
    diag: Option<IXamlDiagnostics>,
    dispatcher: Option<CoreDispatcher>,
    handles: Vec<u64>,
    applied: Option<bool>,
}

struct SyncShared(Mutex<Shared>);
// IXamlDiagnostics and CoreDispatcher are used from the XAML thread or are agile.
unsafe impl Sync for SyncShared {}
unsafe impl Send for SyncShared {}

static SHARED: SyncShared = SyncShared(Mutex::new(Shared { diag: None, dispatcher: None, handles: Vec::new(), applied: None }));

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.0.lock().unwrap_or_else(|e| e.into_inner())
}

fn wanted() -> bool {
    let key: Vec<u16> = "Software\\Nightmoth\0".encode_utf16().collect();
    let name: Vec<u16> = "TaskbarClear\0".encode_utf16().collect();
    let mut v: u32 = 0;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut c_void), Some(&mut size))
    };
    r.is_ok() && v == 1
}

fn element(diag: &IXamlDiagnostics, handle: u64) -> Option<IUIElement> {
    let mut p: *mut c_void = std::ptr::null_mut();
    if unsafe { diag.GetIInspectableFromHandle(handle, &mut p) }.is_err() || p.is_null() {
        return None;
    }
    let insp = unsafe { IInspectable::from_raw(p) };
    insp.cast::<IUIElement>().ok()
}

/// Set opacity only after reading a sane current value back.
fn set_opacity(el: &IUIElement, hide: bool) {
    let mut cur = -1.0f64;
    let ok = unsafe { el.get_Opacity(&mut cur) }.is_ok();
    if ok && cur.is_finite() && (0.0..=1.0).contains(&cur) {
        let _ = unsafe { el.put_Opacity(if hide { 0.0 } else { 1.0 }) };
    }
}

/// On the XAML thread.
fn apply_all(hide: bool) {
    let (diag, handles) = {
        let s = shared();
        (s.diag.clone(), s.handles.clone())
    };
    let Some(diag) = diag else { return };
    for h in handles {
        if let Some(el) = element(&diag, h) {
            set_opacity(&el, hide);
        }
    }
}

fn watcher() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(700));
        let want = wanted();
        let (disp, changed) = {
            let mut s = shared();
            let changed = s.applied.is_some() && s.applied != Some(want);
            s.applied = Some(want);
            (s.dispatcher.clone(), changed)
        };
        if let (true, Some(d)) = (changed, disp) {
            let _ = d.RunAsync(
                CoreDispatcherPriority::Normal,
                &DispatchedHandler::new(move || {
                    apply_all(want);
                    Ok(())
                }),
            );
        }
    });
}

// ------------------------------------------------------------------ COM objects

#[implement(IVisualTreeServiceCallback2, IVisualTreeServiceCallback)]
struct Callback;

impl IVisualTreeServiceCallback_Impl for Callback_Impl {
    unsafe fn OnVisualTreeChange(&self, _relation: *const ParentChildRelation, element_ptr: *const VisualElement, mutation: i32) -> HRESULT {
        if element_ptr.is_null() {
            return HRESULT(0);
        }
        let e = &*element_ptr;
        if mutation == 1 {
            shared().handles.retain(|h| *h != e.handle);
            return HRESULT(0);
        }
        if mutation != 0 {
            return HRESULT(0);
        }
        let name = e.name.to_string();
        if !NAMES.contains(&name.as_str()) {
            return HRESULT(0);
        }
        let ty = e.type_name.to_string();
        if !ty.contains("Rectangle") && !ty.contains("Border") {
            return HRESULT(0);
        }
        let diag = shared().diag.clone();
        let Some(diag) = diag else { return HRESULT(0) };
        let Some(el) = element(&diag, e.handle) else { return HRESULT(0) };
        {
            let mut s = shared();
            if !s.handles.contains(&e.handle) {
                s.handles.push(e.handle);
            }
        }
        if wanted() {
            set_opacity(&el, true);
        }
        HRESULT(0)
    }
}

impl IVisualTreeServiceCallback2_Impl for Callback_Impl {
    unsafe fn OnElementStateChanged(&self, _element: u64, _state: i32, _context: PCWSTR) -> HRESULT {
        HRESULT(0)
    }
}

struct SendSvc(IVisualTreeService);
unsafe impl Send for SendSvc {}

#[implement(IObjectWithSite)]
struct Tap;

impl IObjectWithSite_Impl for Tap_Impl {
    fn SetSite(&self, site: Ref<'_, IUnknown>) -> windows::core::Result<()> {
        let Some(site) = site.as_ref() else { return Ok(()) };
        let diag: IXamlDiagnostics = site.cast()?;
        let service: IVisualTreeService = site.cast()?;
        // the dispatcher of the taskbar's XAML thread, for switching back and forth later
        let mut d: *mut c_void = std::ptr::null_mut();
        let dispatcher = if unsafe { diag.GetDispatcher(&mut d) }.is_ok() && !d.is_null() {
            unsafe { IInspectable::from_raw(d) }.cast::<CoreDispatcher>().ok()
        } else {
            None
        };
        {
            let mut s = shared();
            s.diag = Some(diag);
            s.dispatcher = dispatcher;
        }
        // Advise has to happen off the thread that called SetSite
        let svc = SendSvc(service);
        std::thread::spawn(move || {
            let svc = svc;
            let cb: IVisualTreeServiceCallback = Callback.into();
            let _ = unsafe { svc.0.AdviseVisualTreeChange(cb.as_raw()) };
            std::mem::forget(cb); // Explorer holds it for the rest of its life
        });
        watcher();
        Ok(())
    }

    fn GetSite(&self, _riid: *const GUID, ppv: *mut *mut c_void) -> windows::core::Result<()> {
        if !ppv.is_null() {
            unsafe { *ppv = std::ptr::null_mut() };
        }
        Err(E_NOINTERFACE.into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<'_, IUnknown>, iid: *const GUID, ppv: *mut *mut c_void) -> windows::core::Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unk: IUnknown = Tap.into();
        unsafe { unk.query(iid, ppv).ok() }
    }

    fn LockServer(&self, _lock: windows::core::BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() || clsid.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if *clsid != CLSID_TAP {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let f: IClassFactory = Factory.into();
    f.query(iid, ppv)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    // stays loaded for the life of Explorer
    S_FALSE
}

#[allow(dead_code)]
fn _vtbl(_: &IUnknown_Vtbl) {}
