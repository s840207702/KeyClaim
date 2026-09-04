/*
 * KeyClaim macOS Logitech HID++ bridge.
 * Protocol framing and IOKit callback normalization are adapted from mxswitch
 * by Marco Costa, licensed under the MIT License.
 */
#include <CoreFoundation/CoreFoundation.h>
#include <IOKit/IOKitLib.h>
#include <IOKit/hid/IOHIDManager.h>
#include <IOKit/hidsystem/IOHIDLib.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define LOGITECH_VID 0x046D
#define UNIFYING_PID 0xC52B
#define ROOT_FEATURE 0x00
#define FEATURE_DEVICE_TYPE 0x0005
#define FEATURE_CHANGE_HOST 0x1814
#define DEVICE_TYPE_KEYBOARD 0x00
#define SOFTWARE_ID 0x0A
#define REPORT_SHORT 0x10
#define REPORT_LONG 0x11

typedef struct {
  int current_channel;
  int channel_count;
  int device_index;
  int report_id;
  int feature_index;
  int product_id;
} keyclaim_probe_result;

typedef struct {
  IOHIDManagerRef manager;
  IOHIDDeviceRef device;
  uint8_t device_index;
  uint8_t report_id;
  uint8_t change_host_index;
  int product_id;
  int location_id;
  uint64_t registry_id;
} target_t;

typedef struct {
  int valid;
  uint8_t device_index;
  uint8_t report_id;
  uint8_t change_host_index;
  int product_id;
  int location_id;
  uint64_t registry_id;
} cached_route_t;

static cached_route_t cached_route;

static uint8_t reply_buffer[64];
static CFIndex reply_length;
static int got_reply;

int keyclaim_hid_access_status(void) {
  if (__builtin_available(macOS 10.15, *))
    return (int)IOHIDCheckAccess(kIOHIDRequestTypeListenEvent);
  return (int)kIOHIDAccessTypeGranted;
}

int keyclaim_request_hid_access(void) {
  if (__builtin_available(macOS 10.15, *))
    return IOHIDRequestAccess(kIOHIDRequestTypeListenEvent) ? 1 : 0;
  return 1;
}

static int32_t property_int(IOHIDDeviceRef device, CFStringRef key) {
  CFTypeRef value = IOHIDDeviceGetProperty(device, key);
  int32_t output = 0;
  if (value && CFGetTypeID(value) == CFNumberGetTypeID())
    CFNumberGetValue((CFNumberRef)value, kCFNumberSInt32Type, &output);
  return output;
}

static uint64_t registry_id(IOHIDDeviceRef device) {
  uint64_t output = 0;
  io_service_t service = IOHIDDeviceGetService(device);
  if (service)
    IORegistryEntryGetRegistryEntryID(service, &output);
  return output;
}

static size_t frame_length(uint8_t report_id) {
  return report_id == REPORT_SHORT ? 7 : 20;
}

static void input_callback(void *context, IOReturn result, void *sender,
                           IOHIDReportType type, uint32_t report_id,
                           uint8_t *report, CFIndex length) {
  (void)context;
  (void)result;
  (void)sender;
  (void)type;
  if (length <= 0 || length > (CFIndex)sizeof(reply_buffer) - 1)
    goto done;
  if (report_id != 0 && report[0] != (uint8_t)report_id) {
    reply_buffer[0] = (uint8_t)report_id;
    memcpy(reply_buffer + 1, report, (size_t)length);
    reply_length = length + 1;
  } else {
    memcpy(reply_buffer, report, (size_t)length);
    reply_length = length;
  }
  got_reply = 1;
done:
  CFRunLoopStop(CFRunLoopGetCurrent());
}

static int hidpp_call(IOHIDDeviceRef device, uint8_t report_id,
                      uint8_t device_index, uint8_t feature_index,
                      uint8_t function, const uint8_t *params,
                      size_t param_count, uint8_t *output,
                      double timeout_seconds) {
  uint8_t frame[20];
  size_t length = frame_length(report_id);
  memset(frame, 0, sizeof(frame));
  frame[0] = report_id;
  frame[1] = device_index;
  frame[2] = feature_index;
  frame[3] = (uint8_t)((function << 4) | SOFTWARE_ID);
  if (params && param_count)
    memcpy(frame + 4, params, param_count > 16 ? 16 : param_count);
  if (IOHIDDeviceSetReport(device, kIOHIDReportTypeOutput, report_id, frame,
                           length) != kIOReturnSuccess)
    return 0;
  if (!output)
    return 1;

  double remaining = timeout_seconds;
  while (remaining > 0) {
    got_reply = 0;
    reply_length = 0;
    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, true);
    remaining -= 0.1;
    if (!got_reply || reply_length < 5 || reply_buffer[1] != device_index)
      continue;
    if (reply_buffer[2] == 0xFF || reply_buffer[2] == 0x8F)
      return 0;
    if (reply_buffer[2] != feature_index ||
        (reply_buffer[3] & 0x0F) != SOFTWARE_ID)
      continue;
    memcpy(output, reply_buffer, (size_t)reply_length);
    return 1;
  }
  return 0;
}

static int resolve_feature(IOHIDDeviceRef device, uint8_t report_id,
                           uint8_t device_index, uint16_t feature,
                           uint8_t *feature_index) {
  uint8_t request[3] = {(uint8_t)(feature >> 8), (uint8_t)feature, 0};
  uint8_t response[64];
  if (!hidpp_call(device, report_id, device_index, ROOT_FEATURE, 0, request, 3,
                  response, 0.25) || response[4] == 0)
    return 0;
  *feature_index = response[4];
  return 1;
}

static void release_target(target_t *target) {
  if (target->device) {
    IOHIDDeviceUnscheduleFromRunLoop(target->device, CFRunLoopGetCurrent(),
                                     kCFRunLoopDefaultMode);
    IOHIDDeviceClose(target->device, kIOHIDOptionsTypeNone);
  }
  if (target->manager)
    CFRelease(target->manager);
  memset(target, 0, sizeof(*target));
}

typedef struct {
  IOHIDDeviceRef device;
  int product_id;
  int location_id;
  uint64_t registry_id;
  int rank;
  int max_input;
} candidate_t;

static int compare_candidates(const void *left, const void *right) {
  const candidate_t *a = (const candidate_t *)left;
  const candidate_t *b = (const candidate_t *)right;
  return a->rank - b->rank;
}

static void clear_cached_route(void) {
  memset(&cached_route, 0, sizeof(cached_route));
}

static void remember_target(const target_t *target) {
  cached_route = (cached_route_t){
      .valid = 1,
      .device_index = target->device_index,
      .report_id = target->report_id,
      .change_host_index = target->change_host_index,
      .product_id = target->product_id,
      .location_id = target->location_id,
      .registry_id = target->registry_id,
  };
}

static void prepare_open_device(IOHIDDeviceRef device, int max_input) {
  static uint8_t input_buffer[64];
  IOHIDDeviceRegisterInputReportCallback(device, input_buffer, max_input,
                                         input_callback, NULL);
  IOHIDDeviceScheduleWithRunLoop(device, CFRunLoopGetCurrent(),
                                 kCFRunLoopDefaultMode);
}

static int open_cached_keyboard(target_t *target) {
  memset(target, 0, sizeof(*target));
  if (!cached_route.valid)
    return 0;

  IOHIDManagerRef manager =
      IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
  if (!manager)
    return 0;
  IOHIDManagerSetDeviceMatching(manager, NULL);
  CFSetRef set = IOHIDManagerCopyDevices(manager);
  if (!set) {
    CFRelease(manager);
    return 0;
  }

  CFIndex count = CFSetGetCount(set);
  IOHIDDeviceRef *devices = calloc((size_t)count, sizeof(IOHIDDeviceRef));
  if (!devices && count > 0) {
    CFRelease(set);
    CFRelease(manager);
    return 0;
  }
  CFSetGetValues(set, (const void **)devices);
  int found = 0;
  for (CFIndex i = 0; i < count && !found; i++) {
    IOHIDDeviceRef device = devices[i];
    if (property_int(device, CFSTR(kIOHIDVendorIDKey)) != LOGITECH_VID ||
        property_int(device, CFSTR(kIOHIDProductIDKey)) !=
            cached_route.product_id)
      continue;
    int location_id = property_int(device, CFSTR(kIOHIDLocationIDKey));
    uint64_t candidate_registry_id = registry_id(device);
    if (cached_route.registry_id != 0 &&
        candidate_registry_id != cached_route.registry_id)
      continue;
    if (cached_route.registry_id == 0 && cached_route.location_id != 0 &&
        location_id != cached_route.location_id)
      continue;
    if (IOHIDDeviceOpen(device, kIOHIDOptionsTypeNone) != kIOReturnSuccess)
      continue;
    int max_input = property_int(device, CFSTR(kIOHIDMaxInputReportSizeKey));
    if (max_input <= 0 || max_input > 64)
      max_input = 64;
    prepare_open_device(device, max_input);
    target->manager = manager;
    target->device = device;
    target->device_index = cached_route.device_index;
    target->report_id = cached_route.report_id;
    target->change_host_index = cached_route.change_host_index;
    target->product_id = cached_route.product_id;
    target->location_id = location_id;
    target->registry_id = candidate_registry_id;
    found = 1;
  }
  free(devices);
  CFRelease(set);
  if (!found)
    CFRelease(manager);
  return found;
}

static int find_keyboard(target_t *target) {
  memset(target, 0, sizeof(*target));
  IOHIDManagerRef manager =
      IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
  if (!manager)
    return 0;
  IOHIDManagerSetDeviceMatching(manager, NULL);
  CFSetRef set = IOHIDManagerCopyDevices(manager);
  if (!set) {
    CFRelease(manager);
    return 0;
  }
  CFIndex count = CFSetGetCount(set);
  IOHIDDeviceRef *devices = calloc((size_t)count, sizeof(IOHIDDeviceRef));
  candidate_t *candidates = calloc((size_t)count, sizeof(candidate_t));
  CFSetGetValues(set, (const void **)devices);
  CFIndex candidate_count = 0;
  for (CFIndex i = 0; i < count; i++) {
    IOHIDDeviceRef device = devices[i];
    if (property_int(device, CFSTR(kIOHIDVendorIDKey)) != LOGITECH_VID)
      continue;
    int product_id = property_int(device, CFSTR(kIOHIDProductIDKey));
    int location_id = property_int(device, CFSTR(kIOHIDLocationIDKey));
    uint64_t candidate_registry_id = registry_id(device);
    int max_input = property_int(device, CFSTR(kIOHIDMaxInputReportSizeKey));
    if (max_input <= 0 || max_input > 64)
      max_input = 64;
    candidates[candidate_count++] = (candidate_t){
        .device = device,
        .product_id = product_id,
        .location_id = location_id,
        .registry_id = candidate_registry_id,
        .rank = product_id == UNIFYING_PID ? 0 : 1,
        .max_input = max_input,
    };
  }
  free(devices);
  CFRelease(set);
  qsort(candidates, (size_t)candidate_count, sizeof(candidate_t),
        compare_candidates);

  const uint8_t device_indices[] = {0xFF, 1, 2, 3, 4, 5, 6};
  const uint8_t report_ids[] = {REPORT_LONG, REPORT_SHORT};
  int found = 0;
  for (CFIndex i = 0; i < candidate_count && !found; i++) {
    IOHIDDeviceRef device = candidates[i].device;
    if (IOHIDDeviceOpen(device, kIOHIDOptionsTypeNone) != kIOReturnSuccess)
      continue;
    prepare_open_device(device, candidates[i].max_input);
    for (size_t d = 0; d < sizeof(device_indices) && !found; d++) {
      for (size_t r = 0; r < sizeof(report_ids) && !found; r++) {
        if (candidates[i].product_id != UNIFYING_PID) {
          uint8_t type_index = 0;
          uint8_t type_response[64];
          if (!resolve_feature(device, report_ids[r], device_indices[d],
                               FEATURE_DEVICE_TYPE, &type_index))
            continue;
          if (!hidpp_call(device, report_ids[r], device_indices[d], type_index,
                          2, NULL, 0, type_response, 0.25) ||
              type_response[4] != DEVICE_TYPE_KEYBOARD)
            continue;
        }
        uint8_t change_host_index = 0;
        if (!resolve_feature(device, report_ids[r], device_indices[d],
                             FEATURE_CHANGE_HOST, &change_host_index))
          continue;
        target->manager = manager;
        target->device = device;
        target->device_index = device_indices[d];
        target->report_id = report_ids[r];
        target->change_host_index = change_host_index;
        target->product_id = candidates[i].product_id;
        target->location_id = candidates[i].location_id;
        target->registry_id = candidates[i].registry_id;
        found = 1;
      }
    }
    if (!found) {
      IOHIDDeviceUnscheduleFromRunLoop(device, CFRunLoopGetCurrent(),
                                       kCFRunLoopDefaultMode);
      IOHIDDeviceClose(device, kIOHIDOptionsTypeNone);
    }
  }
  free(candidates);
  if (!found)
    CFRelease(manager);
  return found;
}

int keyclaim_probe_keyboard(keyclaim_probe_result *output) {
  if (!output)
    return 0;
  target_t target;
  int used_cache = open_cached_keyboard(&target);
  if (!used_cache && !find_keyboard(&target))
    return 0;
  uint8_t response[64];
  int success = hidpp_call(target.device, target.report_id,
                           target.device_index, target.change_host_index, 0,
                           NULL, 0, response, 0.4);
  if (!success && used_cache) {
    release_target(&target);
    clear_cached_route();
    if (!find_keyboard(&target))
      return 0;
    success = hidpp_call(target.device, target.report_id,
                         target.device_index, target.change_host_index, 0,
                         NULL, 0, response, 0.4);
  }
  if (success && reply_length >= 6) {
    output->channel_count = response[4];
    output->current_channel = response[5] + 1;
    output->device_index = target.device_index;
    output->report_id = target.report_id;
    output->feature_index = target.change_host_index;
    output->product_id = target.product_id;
    remember_target(&target);
  } else {
    clear_cached_route();
  }
  release_target(&target);
  return success;
}

int keyclaim_switch_keyboard(int channel) {
  if (channel < 1 || channel > 3)
    return 0;
  target_t target;
  int used_cache = open_cached_keyboard(&target);
  if (!used_cache && !find_keyboard(&target))
    return 0;
  uint8_t selected = (uint8_t)(channel - 1);
  int success = hidpp_call(target.device, target.report_id,
                           target.device_index, target.change_host_index, 1,
                           &selected, 1, NULL, 0);
  if (!success && used_cache) {
    release_target(&target);
    clear_cached_route();
    if (!find_keyboard(&target))
      return 0;
    success = hidpp_call(target.device, target.report_id,
                         target.device_index, target.change_host_index, 1,
                         &selected, 1, NULL, 0);
    used_cache = 0;
  }
  release_target(&target);
  clear_cached_route();
  if (!success)
    return 0;
  return used_cache ? 2 : 1;
}
