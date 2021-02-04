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
    #[link(name = "xcb")]
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
    preedit_started: bool,
    last_preedit_length: u32,
}

impl KimeInputContext {
    pub fn new() -> Self {
        Self {
            engine: InputEngine::new(),
            preedit_started: false,
            last_preedit_length: 0,
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

fn make_draw_fr() -> ffi::xcb_im_preedit_draw_fr_t {
    unsafe { std::mem::zeroed() }
}

unsafe fn clear_preedit(
    im: *mut ffi::xcb_im_t,
    xic: *mut ffi::xcb_im_input_context_t,
    ic: &mut KimeInputContext,
) {
    if ffi::xcb_im_input_context_get_input_style(xic) & ffi::XCB_IM_PreeditCallbacks == 0 {
        // TODO: preedit window
        return;
    }

    debug_assert!(ic.preedit_started);

    let mut fr = make_draw_fr();
    fr.chg_length = ic.last_preedit_length;
    fr.status = 1;
    ffi::xcb_im_preedit_draw_callback(im, xic, &mut fr);
    ffi::xcb_im_preedit_done_callback(im, xic);

    ic.preedit_started = false;
    ic.last_preedit_length = 0;
}

unsafe fn update_preedit(
    im: *mut ffi::xcb_im_t,
    xic: *mut ffi::xcb_im_input_context_t,
    ic: &mut KimeInputContext,
    ch: char,
) {
    if ffi::xcb_im_input_context_get_input_style(xic) & ffi::XCB_IM_PreeditCallbacks == 0 {
        // TODO: preedit window
        return;
    }

    if !ic.preedit_started {
        ffi::xcb_im_preedit_start_callback(im, xic);
        ic.preedit_started = true;
    }

    let mut b = [0; 12];
    b[..3].copy_from_slice(UTF8_START);
    let len = ch.len_utf8();
    ch.encode_utf8(&mut b[3..len + 3]);
    b[len + 3..len + 6].copy_from_slice(UTF8_END);

    let mut fr = make_draw_fr();
    fr.chg_length = ic.last_preedit_length;
    fr.preedit_string = b.as_mut_ptr();
    fr.length_of_preedit_string = (len + 6) as _;
    fr.status = 2;
    ic.last_preedit_length = len as _;
    ffi::xcb_im_preedit_draw_callback(im, xic, &mut fr);
}

unsafe fn commit_ch(im: *mut ffi::xcb_im_t, xic: *mut ffi::xcb_im_input_context_t, ch: char) {
    let mut b = [0; 12];
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

unsafe fn commit_ch2(
    im: *mut ffi::xcb_im_t,
    xic: *mut ffi::xcb_im_input_context_t,
    ch1: char,
    ch2: char,
) {
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
                    update_preedit(im, xic, ic, ret.char2);
                }
                InputResultType::Preedit => {
                    update_preedit(im, xic, ic, ret.char1);
                }
                InputResultType::ToggleHangul => {
                    ic.engine.update_hangul_state();
                }
                InputResultType::ClearPreedit => {
                    clear_preedit(im, xic, ic);
                }
            }
        }
        other => {
            log::warn!("Unhandled message: {}", other);
        }
    }
}

unsafe fn main_loop(
    conn: *mut ffi::xcb_connection_t,
    screen_num: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    // ffi::xcb_compound_text_init();

    let (conn, server_win) = {
        let server_win = ffi::xcb_generate_id(conn);
        let screen = ffi::xcb_setup_roots_iterator(ffi::xcb_get_setup(conn)).data;
        let root = (*screen).root;
        let root_visual = (*screen).root_visual;
        ffi::xcb_create_window(
            conn,
            ffi::XCB_COPY_FROM_PARENT as _,
            server_win,
            root,
            0,
            0,
            1,
            1,
            1,
            ffi::XCB_WINDOW_CLASS_INPUT_OUTPUT as _,
            root_visual,
            0,
            ptr::null(),
        );

        (conn, server_win)
    };
    let mut style_arr = [
        ffi::XCB_IM_PreeditPosition | ffi::XCB_IM_StatusNothing,
        ffi::XCB_IM_PreeditPosition | ffi::XCB_IM_StatusNone,
        ffi::XCB_IM_PreeditNothing | ffi::XCB_IM_StatusNothing,
        ffi::XCB_IM_PreeditCallbacks | ffi::XCB_IM_StatusNothing,
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

    let im = ffi::xcb_im_create(
        conn,
        screen_num,
        server_win,
        cs!("kime"),
        ffi::XCB_IM_ALL_LOCALES.as_ptr().cast(),
        &input_styles,
        ptr::null_mut(),
        ptr::null_mut(),
        &encodings,
        1,
        Some(xcb_im_callback),
        (&mut server as *mut KimeServer).cast(),
    );

    if !ffi::xcb_im_open_im(im) {
        return Err("IM open failed".into());
    }
    ffi::xcb_im_set_use_sync_mode(im, true);
    ffi::xcb_im_set_use_sync_event(im, false);

    log::info!("Server initialized, win: {}", server_win);

    loop {
        let e = ffi::xcb_wait_for_event(conn);
        if e.is_null() {
            break;
        }
        let handled = ffi::xcb_im_filter_event(im, e);
        if !handled {
            match (*e).response_type as u32 {
                ffi::XCB_EXPOSE => {}
                ffi::XCB_CONFIGURE_NOTIFY => {}
                e => {
                    log::trace!("Unfiltered event: {:?}", e);
                }
            }
        }
    }

    ffi::xcb_im_close_im(im);
    ffi::xcb_im_destroy(im);

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

    unsafe {
        let mut screen_num = 0;
        let conn = ffi::xcb_connect(ptr::null(), &mut screen_num);
        if let Err(err) = main_loop(conn, screen_num) {
            log::error!("{}", err);
        }
        ffi::xcb_disconnect(conn);
    }
}
