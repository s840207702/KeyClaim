#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>

typedef void (*keyclaim_input_callback)(int32_t delta_x, int32_t delta_y,
                                        int32_t synthetic, void *context);

typedef struct {
  keyclaim_input_callback callback;
  void *context;
} keyclaim_input_monitor;

static LRESULT CALLBACK keyclaim_window_proc(HWND window, UINT message,
                                              WPARAM w_param,
                                              LPARAM l_param) {
  keyclaim_input_monitor *monitor = (keyclaim_input_monitor *)GetWindowLongPtrW(
      window, GWLP_USERDATA);

  if (message == WM_NCCREATE) {
    CREATESTRUCTW *create = (CREATESTRUCTW *)l_param;
    monitor = (keyclaim_input_monitor *)create->lpCreateParams;
    SetWindowLongPtrW(window, GWLP_USERDATA, (LONG_PTR)monitor);
  } else if (message == WM_INPUT && monitor && monitor->callback) {
    RAWINPUT input;
    UINT size = sizeof(input);
    if (GetRawInputData((HRAWINPUT)l_param, RID_INPUT, &input, &size,
                        sizeof(RAWINPUTHEADER)) != (UINT)-1 &&
        input.header.dwType == RIM_TYPEMOUSE) {
      int32_t delta_x = input.data.mouse.lLastX;
      int32_t delta_y = input.data.mouse.lLastY;
      USHORT buttons = input.data.mouse.usButtonFlags;
      if (delta_x != 0 || delta_y != 0 ||
          (buttons & (RI_MOUSE_WHEEL | RI_MOUSE_HWHEEL)) != 0) {
        if (delta_x == 0 && delta_y == 0)
          delta_y = 1;
        monitor->callback(delta_x, delta_y, 0, monitor->context);
      }
    }
  } else if (message == WM_CLOSE) {
    DestroyWindow(window);
    return 0;
  } else if (message == WM_DESTROY) {
    PostQuitMessage(0);
    return 0;
  }

  return DefWindowProcW(window, message, w_param, l_param);
}

int32_t keyclaim_run_input_monitor(keyclaim_input_callback callback,
                                   void *context) {
  if (!callback)
    return 0;

  static const wchar_t class_name[] = L"KeyClaimRawInputWindowV2";
  HINSTANCE instance = GetModuleHandleW(NULL);
  WNDCLASSW window_class = {0};
  window_class.lpfnWndProc = keyclaim_window_proc;
  window_class.hInstance = instance;
  window_class.lpszClassName = class_name;
  if (!RegisterClassW(&window_class) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS)
    return 0;

  keyclaim_input_monitor monitor = {0};
  monitor.callback = callback;
  monitor.context = context;
  HWND window = CreateWindowExW(0, class_name, L"", 0, 0, 0, 0, 0,
                                HWND_MESSAGE, NULL, instance, &monitor);
  if (!window)
    return 0;

  RAWINPUTDEVICE device = {0};
  device.usUsagePage = 0x01;
  device.usUsage = 0x02;
  device.dwFlags = RIDEV_INPUTSINK;
  device.hwndTarget = window;
  if (!RegisterRawInputDevices(&device, 1, sizeof(device))) {
    DestroyWindow(window);
    return 0;
  }

  callback(0, 0, -1, context);

  MSG message;
  while (GetMessageW(&message, NULL, 0, 0) > 0) {
    TranslateMessage(&message);
    DispatchMessageW(&message);
  }
  return 1;
}
