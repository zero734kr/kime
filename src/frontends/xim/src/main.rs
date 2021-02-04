use kime_engine_cffi::{
    Config, InputEngine, InputResultType, MODIFIER_CONTROL, MODIFIER_SHIFT, MODIFIER_SUPER,
};

use std::ffi::c_void;
use std::ptr;

mod ffi {
    #![allow(non_camel_case_types)]
    #![allow(non_upper_case_globals)]
    #![allow(non_snake_case)]
    #![allow(dead_code)]

    #[link(name = "xcb-imdkit")]
    extern "C" {}

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

// mod pe_window;

macro_rules! cs {
    ($ex:expr) => {
        concat!($ex, "\0").as_ptr().cast()
    };
}

struct KimeInputContext {
    engine: InputEngine,
}

impl KimeInputContext {
    pub fn new() -> Self {
        Self {
            engine: InputEngine::new(),
        }
    }
}

struct KimeServer {
    config: Config,
}

impl KimeServer {
    pub fn new() -> Self {
        Self {
            config: Config::new(),
        }
    }
}

const UTF8_START: &[u8] = &[0x1B, 0x25, 0x47];
const UTF8_END: &[u8] = &[0x1B, 0x25, 0x40];

fn commit_ch(im: *mut ffi::xcb_im_t, xic: *mut ffi::xcb_im_input_context_t, ch: char) {
    unsafe {
        let mut b = [0; 12];
        // let s = ch.encode_utf8(&mut b);
        // let mut c_len = 0;
        // let c_s = ffi::xcb_utf8_to_compound_text(s.as_ptr().cast(), s.len() as _, &mut c_len);
        // ffi::xcb_im_commit_string(im, xic, ffi::XCB_XIM_LOOKUP_CHARS, c_s, c_len as _, 0);
        // libc::free(c_s.cast());
        b[..3].copy_from_slice(UTF8_START);
        let len = ch.len_utf8();
        ch.encode_utf8(&mut b[3..len + 3]);
        b[len + 3..len + 6].copy_from_slice(UTF8_END);
        ffi::xcb_im_commit_string(
            im,
            xic,
            ffi::XCB_XIM_LOOKUP_CHARS,
            b.as_ptr().cast(),
            (len + 6) as _,
            0,
        );
    }
}

fn commit_ch2(im: *mut ffi::xcb_im_t, xic: *mut ffi::xcb_im_input_context_t, ch1: char, ch2: char) {
    unsafe {
        let len1 = ch1.len_utf8();
        let len = len1 + ch2.len_utf8();
        let mut b = [0; 16];
        b[..3].copy_from_slice(UTF8_START);
        ch1.encode_utf8(&mut b[3..len1 + 3]);
        ch2.encode_utf8(&mut b[len1 + 3..len + 3]);
        b[len + 3..len + 6].copy_from_slice(UTF8_END);
        ffi::xcb_im_commit_string(
            im,
            xic,
            ffi::XCB_XIM_LOOKUP_CHARS,
            b.as_ptr().cast(),
            (len + 6) as _,
            0,
        );
    }
}

unsafe extern "C" fn xcb_im_callback(
    im: *mut ffi::xcb_im_t,
    _client: *mut ffi::xcb_im_client_t,
    xic: *mut ffi::xcb_im_input_context_t,
    hdr: *const ffi::xcb_im_packet_header_fr_t,
    _frame: *mut c_void,
    arg: *mut c_void,
    user_data: *mut c_void,
) {
    let server = if let Some(server) = user_data.cast::<KimeServer>().as_mut() {
        server
    } else {
        return;
    };

    if xic.is_null() {
        return;
    }

    let ic = ffi::xcb_im_input_context_get_data(xic).cast::<KimeInputContext>();

    if (*hdr).major_opcode == ffi::XCB_XIM_CREATE_IC {
        ffi::xcb_im_input_context_set_data(
            xic,
            Box::into_raw(Box::new(KimeInputContext::new())).cast(),
            None,
        );
        return;
    }

    let ic = if let Some(ic) = ic.as_mut() {
        ic
    } else {
        return;
    };

    match (*hdr).major_opcode {
        ffi::XCB_XIM_DESTROY_IC => {
            ffi::xcb_im_input_context_set_data(xic, ptr::null_mut(), None);
            let _ = Box::from_raw(ic);
        }
        ffi::XCB_XIM_SET_IC_VALUES => {
            // TODO: update preedit spot
        }
        ffi::XCB_XIM_GET_IC_VALUES => {}
        ffi::XCB_XIM_SET_IC_FOCUS => {
            ic.engine.update_hangul_state();
        }
        ffi::XCB_XIM_RESET_IC | ffi::XCB_XIM_UNSET_IC_FOCUS => {
            if let Some(ch) = ic.engine.reset() {
                commit_ch(im, xic, ch);
            }
        }
        ffi::XCB_XIM_FORWARD_EVENT => {
            let xev = arg.cast::<ffi::xcb_key_press_event_t>();
            let mut state = 0;

            if (*xev).state & 0x1 != 0 {
                state |= MODIFIER_SHIFT;
            }

            if (*xev).state & 0x4 != 0 {
                state |= MODIFIER_CONTROL;
            }

            if (*xev).state & 0x40 != 0 {
                state |= MODIFIER_SUPER;
            }

            let ret = ic
                .engine
                .press_key(&server.config, (*xev).detail as _, state);

            log::trace!("{:?}", ret);

            match ret.ty {
                InputResultType::Bypass => {
                    ffi::xcb_im_forward_event(im, xic, xev);
                }
                InputResultType::Commit => {
                    commit_ch(im, xic, ret.char1);
                }
                InputResultType::CommitBypass => {
                    commit_ch(im, xic, ret.char1);
                    ffi::xcb_im_forward_event(im, xic, xev);
                }
                InputResultType::CommitCommit => {
                    commit_ch2(im, xic, ret.char1, ret.char2);
                }
                InputResultType::CommitPreedit => {
                    commit_ch(im, xic, ret.char1);
                    // preedit
                }
                InputResultType::Preedit => {
                    // preedit
                }
                InputResultType::ToggleHangul => {
                    ic.engine.update_hangul_state();
                }
                InputResultType::ClearPreedit => {
                    // clear preedit
                }
            }
        }
        other => {
            log::warn!("Unhandled message: {}", other);
        }
    }
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        ffi::xcb_compound_text_init();
    }

    let (conn, screen_num) = xcb::Connection::connect(None)?;
    let screen = conn.get_setup().roots().nth(screen_num as usize).unwrap();
    let server_win = conn.generate_id();
    xcb::create_window(
        &conn,
        xcb::COPY_FROM_PARENT as _,
        server_win,
        screen.root(),
        0,
        0,
        1,
        1,
        1,
        xcb::WINDOW_CLASS_INPUT_OUTPUT as _,
        screen.root_visual(),
        &[],
    )
    .request_check()?;
    let mut style_arr = [
        ffi::XCB_IM_PreeditPosition | ffi::XCB_IM_StatusArea,
        ffi::XCB_IM_PreeditPosition | ffi::XCB_IM_StatusNothing,
        ffi::XCB_IM_PreeditPosition | ffi::XCB_IM_StatusNone,
        ffi::XCB_IM_PreeditNothing | ffi::XCB_IM_StatusNothing,
        ffi::XCB_IM_PreeditNone | ffi::XCB_IM_StatusNone,
    ];
    let mut encoding_arr = [(b"COMPOUND_TEXT\0".as_ptr() as *mut u8).cast()];
    let encodings = ffi::xcb_im_encodings_t {
        nEncodings: encoding_arr.len() as _,
        encodings: encoding_arr.as_mut_ptr(),
    };
    let input_styles = ffi::xcb_im_styles_t {
        nStyles: style_arr.len() as _,
        styles: style_arr.as_mut_ptr(),
    };

    let mut server = KimeServer::new();

    let im = unsafe {
        ffi::xcb_im_create(
            conn.get_raw_conn().cast(),
            screen_num as _,
            server_win,
            cs!("kime_test"),
            ffi::XCB_IM_ALL_LOCALES.as_ptr().cast(),
            &input_styles,
            ptr::null_mut(),
            ptr::null_mut(),
            &encodings,
            1,
            Some(xcb_im_callback),
            (&mut server as *mut KimeServer).cast(),
        )
    };

    unsafe {
        if !ffi::xcb_im_open_im(im) {
            return Err("IM open failed".into());
        }
        ffi::xcb_im_set_use_sync_mode(im, true);
        ffi::xcb_im_set_use_sync_event(im, false);
    }

    log::info!("Server initialized, win: {}", server_win);

    while let Some(e) = conn.wait_for_event() {
        let handled = unsafe { ffi::xcb_im_filter_event(im, e.ptr.cast()) };

        if !handled {
            match e.response_type() {
                xcb::EXPOSE => {}
                xcb::CONFIGURE_NOTIFY => {}
                e => {
                    log::trace!("Unfiltered event: {:?}", e);
                }
            }
        }
    }

    unsafe {
        ffi::xcb_im_close_im(im);
        ffi::xcb_im_destroy(im);
    }

    log::info!("Server exited");

    Ok(())
}

fn main() {
    let mut args = pico_args::Arguments::from_env();

    if args.contains(["-h", "--help"]) {
        println!("-h or --help: show help");
        println!("-v or --version: show version");
        println!("--verbose: more verbose log");
        return;
    }

    if args.contains(["-v", "--version"]) {
        kime_version::print_version!();
        return;
    }

    let mut log_level = if cfg!(debug_assertions) {
        log::LevelFilter::Trace
    } else {
        log::LevelFilter::Info
    };

    if args.contains("--verbose") {
        log_level = log::LevelFilter::Trace;
    }

    simplelog::SimpleLogger::init(log_level, simplelog::ConfigBuilder::new().build()).unwrap();

    log::info!("Start xim server version: {}", env!("CARGO_PKG_VERSION"));

    if let Err(err) = main_loop() {
        log::error!("{}", err);
    }
}
