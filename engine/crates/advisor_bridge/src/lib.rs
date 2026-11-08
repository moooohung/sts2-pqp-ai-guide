//! Single-worker C ABI. All pointers are caller-owned, readable for the stated
//! length, and handles must not be used concurrently or after destruction.
use std::{ffi::{CString,c_char},panic::{AssertUnwindSafe,catch_unwind},slice,str};
use sts2_aiplay::native::live::Advisor;
const SOURCE_COMMIT:&str=match option_env!("STS2_ADVISOR_SOURCE_COMMIT"){
    Some(commit)=>commit,None=>"UNATTESTED",
};

#[unsafe(no_mangle)]
pub extern "C" fn sts2_advisor_source_commit()->*const c_char{
    static TEXT:std::sync::OnceLock<CString>=std::sync::OnceLock::new();
    TEXT.get_or_init(||CString::new(SOURCE_COMMIT).unwrap()).as_ptr()
}

pub struct Handle {advisor:Option<Advisor>,response:CString}
fn text(value:String)->CString {
    CString::new(value).unwrap_or_else(|_|CString::new(r#"{"ok":false,"error":{"code":"INVALID_RESPONSE"}}"#).unwrap())
}
/// Returns a handle even on refusal; call `request` to read that refusal.
/// Null means construction panicked or the supplied sizes exceed the ABI limits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sts2_advisor_create(weights:*const u8,len:usize,start:*const u8,start_len:usize)->*mut Handle {
    if weights.is_null()||start.is_null()||len>64<<20||start_len>16<<20{return std::ptr::null_mut();}
    catch_unwind(AssertUnwindSafe(||{
        let result=Advisor::new(unsafe{slice::from_raw_parts(weights,len)},unsafe{slice::from_raw_parts(start,start_len)});
        let (advisor,response)=match result {
            Ok(mut advisor)=>{let response=advisor.request(r#"{"op":"meta"}"#);(Some(advisor),response)},
            Err(reason)=>(None,sts2_aiplay::native::live::initialization_refusal(&reason)),
        };
        Box::into_raw(Box::new(Handle{advisor,response:text(response)}))
    })).unwrap_or(std::ptr::null_mut())
}
/// UTF-8 response remains valid until the next request or destroy on this handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sts2_advisor_request(handle:*mut Handle,request:*const u8,len:usize)->*const c_char {
    if handle.is_null()||request.is_null()||len>16<<20{return std::ptr::null();}
    let handle=unsafe{&mut *handle};
    let result=catch_unwind(AssertUnwindSafe(||{
        let request=str::from_utf8(unsafe{slice::from_raw_parts(request,len)})
            .map_err(|_|())?;
        if let Some(advisor)=&mut handle.advisor {handle.response=text(advisor.request(request));}
        Ok::<(),()>(())
    }));
    if !matches!(result,Ok(Ok(()))) {
        handle.advisor=None;
        handle.response=text(r#"{"ok":false,"replay_clean":false,"error":{"code":"NATIVE_REQUEST_FAULT"}}"#.into());
    }
    handle.response.as_ptr()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sts2_advisor_destroy(handle:*mut Handle) {
    if !handle.is_null(){drop(unsafe{Box::from_raw(handle)});}
}
