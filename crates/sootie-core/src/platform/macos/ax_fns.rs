use core_foundation::array::CFArray;
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};

pub type AXError = i32;
pub const K_AX_ERROR_SUCCESS: AXError = 0;
pub const K_AX_ERROR_API_DISABLED: AXError = -25200;
pub const K_AX_ERROR_NOT_IMPLEMENTED: AXError = -25201;
pub const K_AX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;

pub type AXUIElementRef = *mut std::ffi::c_void;
pub type CFTypeID = std::ffi::c_ulong;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

pub const K_AX_VALUE_CGPOINT_TYPE: i32 = 2;
pub const K_AX_VALUE_CGSIZE_TYPE: i32 = 3;

pub type AXValueRef = *mut std::ffi::c_void;

extern "C" {
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXValueGetValue(value: AXValueRef, type_: i32, value_ptr: *mut std::ffi::c_void)
        -> bool;
    pub fn AXIsProcessTrusted() -> bool;
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFBooleanGetTypeID() -> CFTypeID;
    pub fn CFArrayGetTypeID() -> CFTypeID;
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
}

pub fn cfstr(s: &str) -> CFString {
    CFString::new(s)
}

pub fn is_process_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub unsafe fn retain_ax_element(element: AXUIElementRef) -> AXUIElementRef {
    if !element.is_null() {
        CFRetain(element as CFTypeRef);
    }
    element
}

pub unsafe fn release_ax_element(element: AXUIElementRef) {
    if !element.is_null() {
        CFRelease(element as CFTypeRef);
    }
}

unsafe fn get_type_id(value: CFTypeRef) -> CFTypeID {
    if value.is_null() {
        return 0;
    }
    CFGetTypeID(value)
}

pub unsafe fn get_string_attr(element: AXUIElementRef, attr: &str) -> Option<String> {
    let cf_attr = cfstr(attr);
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value);
    if err != K_AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }

    let type_id = get_type_id(value);
    let string_type_id = CFStringGetTypeID();
    if type_id != string_type_id {
        return None;
    }

    let cf_str: CFString = TCFType::wrap_under_create_rule(std::mem::transmute(value));
    Some(cf_str.to_string())
}

pub unsafe fn get_bool_attr(element: AXUIElementRef, attr: &str) -> Option<bool> {
    let cf_attr = cfstr(attr);
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value);
    if err != K_AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }

    let type_id = get_type_id(value);
    let bool_type_id = CFBooleanGetTypeID();
    if type_id != bool_type_id {
        return None;
    }

    let cf_bool: CFBoolean = TCFType::wrap_under_create_rule(std::mem::transmute(value));
    Some(cf_bool.into())
}

pub unsafe fn get_point_attr(element: AXUIElementRef, attr: &str) -> Option<CGPoint> {
    let cf_attr = cfstr(attr);
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value);
    if err != K_AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    if AXValueGetValue(
        std::mem::transmute(value),
        K_AX_VALUE_CGPOINT_TYPE,
        &mut point as *mut CGPoint as *mut std::ffi::c_void,
    ) {
        Some(point)
    } else {
        None
    }
}

pub unsafe fn get_size_attr(element: AXUIElementRef, attr: &str) -> Option<CGSize> {
    let cf_attr = cfstr(attr);
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value);
    if err != K_AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    if AXValueGetValue(
        std::mem::transmute(value),
        K_AX_VALUE_CGSIZE_TYPE,
        &mut size as *mut CGSize as *mut std::ffi::c_void,
    ) {
        Some(size)
    } else {
        None
    }
}

pub unsafe fn get_children(element: AXUIElementRef) -> Vec<AXUIElementRef> {
    let cf_attr = cfstr("AXChildren");
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value);
    if err != K_AX_ERROR_SUCCESS || value.is_null() {
        return vec![];
    }

    let type_id = get_type_id(value);
    let array_type_id = CFArrayGetTypeID();
    if type_id != array_type_id {
        return vec![];
    }

    let cf_array: CFArray<CFTypeRef> = TCFType::wrap_under_create_rule(std::mem::transmute(value));
    cf_array
        .iter()
        .map(|p| retain_ax_element(*p as AXUIElementRef))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfstr_basic() {
        let s = cfstr("test");
        assert_eq!(s.to_string(), "test");
    }

    #[test]
    fn test_cfstr_empty() {
        let s = cfstr("");
        assert_eq!(s.to_string(), "");
    }

    #[test]
    fn test_cfstr_with_spaces() {
        let s = cfstr("hello world");
        assert_eq!(s.to_string(), "hello world");
    }

    #[test]
    fn test_cgpoint_struct() {
        let point = CGPoint { x: 100.0, y: 200.0 };
        assert_eq!(point.x, 100.0);
        assert_eq!(point.y, 200.0);
    }

    #[test]
    fn test_cgsize_struct() {
        let size = CGSize {
            width: 50.0,
            height: 75.0,
        };
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 75.0);
    }

    #[test]
    fn test_constants() {
        assert_eq!(K_AX_ERROR_SUCCESS, 0);
        assert_eq!(K_AX_VALUE_CGPOINT_TYPE, 2);
        assert_eq!(K_AX_VALUE_CGSIZE_TYPE, 3);
    }

    #[test]
    fn test_cgpoint_negative() {
        let point = CGPoint { x: -10.0, y: -20.0 };
        assert_eq!(point.x, -10.0);
        assert_eq!(point.y, -20.0);
    }

    #[test]
    fn test_cgsize_zero() {
        let size = CGSize {
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(size.width, 0.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn test_cfstr_special_chars() {
        let s = cfstr("test-with-dashes");
        assert_eq!(s.to_string(), "test-with-dashes");
    }
}
