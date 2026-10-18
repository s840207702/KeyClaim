#include "../src-tauri/native/logitech_hidpp_macos.m"
#include <malloc/malloc.h>
#include <stdio.h>
#include <unistd.h>

static size_t heap_bytes(void) {
  malloc_statistics_t stats = {0};
  malloc_zone_statistics(NULL, &stats);
  return stats.size_in_use;
}

static int probe_once(void) {
  keyclaim_probe_result result = {0};
  return keyclaim_probe_keyboard(&result);
}

int main(void) {
  if (keyclaim_hid_access_status() != 0) {
    fputs("未运行内存检查：当前终端未获输入监控权限。\n", stderr);
    return 77;
  }
  if (!probe_once()) {
    fputs("未运行内存检查：未检测到在线的受支持键盘，请确认接收器连接并唤醒键盘。\n", stderr);
    return 77;
  }

  // Deliberately do not add an outer autorelease pool: the native entry point
  // must manage its own temporary objects on a long-lived Rust worker thread.
  const int warmup = 50;
  const int iterations = 500;
  const size_t limit = 4 * 1024 * 1024;
  size_t before = 0;
  for (int i = 1; i <= warmup + iterations; i++) {
    if (!probe_once()) {
      fputs("内存检查中止：键盘探测失败，不能作为通过结果。\n", stderr);
      return 77;
    }
    if (i == warmup)
      before = heap_bytes();
    usleep(20000);
  }
  size_t after = heap_bytes();
  size_t growth = after > before ? after - before : 0;
  printf("成功探测 %d 次（另有 %d 次预热）；堆内存 %.3f → %.3f MiB，增长 %.3f MiB，允许最多 %.1f MiB。\n",
         iterations, warmup, before / 1048576.0, after / 1048576.0,
         growth / 1048576.0, limit / 1048576.0);
  if (growth > limit) {
    fputs("FAIL：重复硬件探测仍在积累内存。\n", stderr);
    return 1;
  }
  puts("PASS：重复硬件探测的内存增长在限制内。此检查不切换键盘通道。");
  return 0;
}
