#import <AppKit/AppKit.h>
#include <stdint.h>

int keyclaim_get_cursor_position(int32_t *x, int32_t *y) {
  if (!x || !y)
    return 0;

  NSPoint location = [NSEvent mouseLocation];
  *x = (int32_t)location.x;
  *y = (int32_t)location.y;
  return 1;
}
