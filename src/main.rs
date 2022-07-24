use std::sync::mpsc;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::Globalization,
    Win32::{Globalization::MAX_LOCALE_NAME, System::LibraryLoader::GetModuleHandleW},
    Win32::{
        System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED},
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{self, *},
        },
    },
};

fn main() {
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED).unwrap();

        let instance = GetModuleHandleW(None).unwrap();
        debug_assert!(instance.0 != 0);

        let window_class = PCWSTR("Window Class".encode_utf16().collect::<Vec<u16>>().as_ptr());

        let wc = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hInstance: instance,
            lpszClassName: window_class,

            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            ..Default::default()
        };

        let atom = RegisterClassW(&wc);
        debug_assert!(atom != 0);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            window_class,
            "WebView2 Test",
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            std::ptr::null(),
        );

        let env = create_env().unwrap();
        let ctrl = create_controller(hwnd, &env).unwrap();
        init_webview(hwnd, &ctrl).unwrap();

        let mut message = MSG::default();

        while GetMessageW(&mut message, HWND(0), 0, 0).into() {
            DispatchMessageW(&message);
        }
    }
}

fn create_env() -> webview2_com::Result<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();

    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |environmentcreatedhandler| unsafe {
            let options = {
                let options: ICoreWebView2EnvironmentOptions =
                    CoreWebView2EnvironmentOptions::default().into();

                // Setting user's system language
                let lcid = Globalization::GetUserDefaultUILanguage();
                let mut lang = [0; MAX_LOCALE_NAME as usize];
                Globalization::LCIDToLocaleName(
                    lcid as u32,
                    &mut lang,
                    Globalization::LOCALE_ALLOW_NEUTRAL_NAMES,
                );
                println!("UI Language: {}", String::from_utf16_lossy(&lang));
                options
                    .SetLanguage(PCWSTR(lang.as_ptr()))
                    .map_err(webview2_com::Error::WindowsError)?;
                options
            };

            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::default(),
                PCWSTR::default(),
                options,
                environmentcreatedhandler,
            )
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send over mpsc channel");
            Ok(())
        }),
    )?;

    rx.recv()
        .map_err(|_| webview2_com::Error::SendError)?
        .map_err(webview2_com::Error::WindowsError)
}

fn create_controller(
    hwnd: HWND,
    env: &ICoreWebView2Environment,
) -> webview2_com::Result<ICoreWebView2Controller> {
    let (tx, rx) = mpsc::channel();
    let env = env.clone();

    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            env.CreateCoreWebView2Controller(hwnd, handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send over mpsc channel");
            Ok(())
        }),
    )?;

    rx.recv()
        .map_err(|_| webview2_com::Error::SendError)?
        .map_err(webview2_com::Error::WindowsError)
}

fn init_webview(
    hwnd: HWND,
    controller: &ICoreWebView2Controller,
) -> webview2_com::Result<ICoreWebView2> {
    let webview =
        unsafe { controller.CoreWebView2() }.map_err(webview2_com::Error::WindowsError)?;

    unsafe {
        let settings = webview
            .Settings()
            .map_err(webview2_com::Error::WindowsError)?;
        settings
            .SetAreDevToolsEnabled(true)
            .map_err(webview2_com::Error::WindowsError)?;

        let mut rect = RECT::default();
        WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
        controller
            .SetBounds(rect)
            .map_err(webview2_com::Error::WindowsError)?;
    }

    let html = "
    <div></div>
    <script>
      const div = document.querySelector('div');
      div.textContent = window.navigator.languages.join(' ');
    </script>
";
    unsafe {
        webview
            .NavigateToString(html)
            .map_err(webview2_com::Error::WindowsError)?;
    }

    unsafe {
        unsafe extern "system" fn subclass_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
            _uidsubclass: usize,
            dwrefdata: usize,
        ) -> LRESULT {
            if msg == WindowsAndMessaging::WM_SIZE {
                let controller = dwrefdata as *mut ICoreWebView2Controller;
                let mut client_rect = RECT::default();
                WindowsAndMessaging::GetClientRect(hwnd, std::mem::transmute(&mut client_rect));
                let _ = (*controller).SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: client_rect.right - client_rect.left,
                    bottom: client_rect.bottom - client_rect.top,
                });
            }

            if msg == WindowsAndMessaging::WM_DESTROY {
                Box::from_raw(dwrefdata as *mut ICoreWebView2Controller);
            }

            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        SetWindowSubclass(
            hwnd,
            Some(subclass_proc),
            8080,
            Box::into_raw(Box::new(controller.clone())) as _,
        );
    }

    unsafe {
        controller
            .SetIsVisible(true)
            .map_err(webview2_com::Error::WindowsError)?;
        controller
            .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
            .map_err(webview2_com::Error::WindowsError)?;
    }

    Ok(webview)
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message as u32 {
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
