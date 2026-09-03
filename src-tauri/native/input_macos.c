#include <ApplicationServices/ApplicationServices.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdint.h>

typedef void (*keyclaim_input_callback)(int32_t delta_x, int32_t delta_y,
                                        int32_t synthetic, void *context);

typedef struct {
  keyclaim_input_callback callback;
  void *context;
  CFMachPortRef tap;
  CGPoint last_location;
  int has_last_location;
} keyclaim_input_monitor;

static CGEventRef input_event_callback(CGEventTapProxy proxy, CGEventType type,
                                       CGEventRef event, void *user_info) {
  (void)proxy;
  keyclaim_input_monitor *monitor = (keyclaim_input_monitor *)user_info;
  if (!monitor)
    return event;

  if (type == kCGEventTapDisabledByTimeout ||
      type == kCGEventTapDisabledByUserInput) {
    if (monitor->tap)
      CGEventTapEnable(monitor->tap, true);
    return event;
  }

  CGPoint location = CGEventGetLocation(event);
  int32_t delta_x = (int32_t)CGEventGetIntegerValueField(
      event, kCGMouseEventDeltaX);
  int32_t delta_y = (int32_t)CGEventGetIntegerValueField(
      event, kCGMouseEventDeltaY);

  if (delta_x == 0 && delta_y == 0 && monitor->has_last_location) {
    delta_x = (int32_t)(location.x - monitor->last_location.x);
    delta_y = (int32_t)(location.y - monitor->last_location.y);
  }
  monitor->last_location = location;
  monitor->has_last_location = 1;

  if (type == kCGEventScrollWheel && delta_x == 0 && delta_y == 0) {
    delta_y = (int32_t)CGEventGetIntegerValueField(
        event, kCGScrollWheelEventPointDeltaAxis1);
    delta_x = (int32_t)CGEventGetIntegerValueField(
        event, kCGScrollWheelEventPointDeltaAxis2);
  }

  if (delta_x == 0 && delta_y == 0)
    return event;

  int64_t source_pid = CGEventGetIntegerValueField(
      event, kCGEventSourceUnixProcessID);
  int32_t synthetic = source_pid > 0 ? 1 : 0;
  monitor->callback(delta_x, delta_y, synthetic, monitor->context);
  return event;
}

int32_t keyclaim_run_input_monitor(keyclaim_input_callback callback,
                                   void *context) {
  if (!callback)
    return 0;

  CGEventMask mask = CGEventMaskBit(kCGEventMouseMoved) |
                     CGEventMaskBit(kCGEventLeftMouseDragged) |
                     CGEventMaskBit(kCGEventRightMouseDragged) |
                     CGEventMaskBit(kCGEventOtherMouseDragged) |
                     CGEventMaskBit(kCGEventScrollWheel);
  keyclaim_input_monitor monitor = {
      .callback = callback,
      .context = context,
      .tap = NULL,
      .last_location = CGPointZero,
      .has_last_location = 0,
  };

  CFMachPortRef tap = CGEventTapCreate(
      kCGHIDEventTap, kCGHeadInsertEventTap, kCGEventTapOptionListenOnly, mask,
      input_event_callback, &monitor);
  if (!tap)
    return 0;
  monitor.tap = tap;

  CFRunLoopSourceRef source =
      CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0);
  if (!source) {
    CFRelease(tap);
    return 0;
  }

  CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
  CGEventTapEnable(tap, true);
  callback(0, 0, -1, context);
  CFRunLoopRun();

  CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
  CFRelease(source);
  CFRelease(tap);
  return 1;
}
