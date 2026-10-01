/* Owned-VM acceptance peer, not a product input path or portal bypass.
 * Only six fixed composition edges are accepted. No event content is exported.
 * Compile against the pinned libei headers/library used by native acceptance.
 */
#define _POSIX_C_SOURCE 200809L
#include <libei.h>
#include <linux/input-event-codes.h>
#include <sys/stat.h>
#include <sys/socket.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t cancelled;
static void cancel_signal(int signum) { (void)signum; cancelled = 1; }
static int64_t now_ms(void) {
  struct timespec t;
  if (clock_gettime(CLOCK_MONOTONIC, &t) != 0) return -1;
  return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

int main(int argc, char **argv) {
  const char *uuid = "812400f8-a6c6-4c38-a735-2b8d0ef8d3e2";
  if (argc != 5 || strcmp(argv[1], "--owned-vm-uuid") || strcmp(argv[2], uuid) ||
      strcmp(argv[3], "--fd") || getuid() != 1000 || geteuid() != 1000) return 2;
  int marker = open("/etc/cu-owned-vm-id", O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
  struct stat st;
  char marker_value[64] = {0};
  if (marker < 0 || fstat(marker, &st) || !S_ISREG(st.st_mode) || st.st_uid != 0 ||
      (st.st_mode & 0222) || read(marker, marker_value, sizeof(marker_value) - 1) != 36 ||
      memcmp(marker_value, uuid, 36)) return 2;
  close(marker);
  char *end;
  long raw_fd = strtol(argv[4], &end, 10);
  if (*end || raw_fd < 3 || raw_fd > 65535) return 2;
  int fd = (int)raw_fd;
  struct sockaddr_storage addr;
  socklen_t len = sizeof(addr);
  if (getsockname(fd, (struct sockaddr *)&addr, &len) || addr.ss_family != AF_UNIX ||
      fstat(fd, &st) || !S_ISSOCK(st.st_mode)) return 2;
  int flags = fcntl(fd, F_GETFL);
  if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK)) return 2;
  struct sigaction sa = {0};
  sa.sa_handler = cancel_signal;
  sigemptyset(&sa.sa_mask);
  if (sigaction(SIGTERM, &sa, NULL) || sigaction(SIGINT, &sa, NULL)) return 2;
  signal(SIGPIPE, SIG_IGN);
  setvbuf(stdout, NULL, _IONBF, 0);

  struct ei *context = ei_new_sender(NULL);
  if (!context) { close(fd); return 3; }
  ei_configure_name(context, "Grok owned native IME acceptance");
  if (ei_setup_backend_fd(context, fd) != 0) { ei_unref(context); return 3; }
  struct ei_device *keyboard = NULL;
  bool ready = false, started = false, connected = false, stop = false;
  bool pressed[3] = {false, false, false};
  const uint32_t keys[] = {KEY_N, KEY_I, KEY_SPACE};
  unsigned int sequence = 0, seats = 0;
  int code = 0;
  int64_t begun = now_ms();
  char command[16];
  size_t used = 0;
  while (!stop && !cancelled) {
    if (now_ms() < 0 || now_ms() - begun > (ready ? 90000 : 10000)) { code = 4; break; }
    ei_dispatch(context);
    struct ei_event *event;
    while ((event = ei_get_event(context))) {
      enum ei_event_type type = ei_event_get_type(event);
      struct ei_device *device;
      switch (type) {
      case EI_EVENT_CONNECT:
        if (connected) code = 5;
        connected = true;
        break;
      case EI_EVENT_DISCONNECT:
        code = 5;
        break;
      case EI_EVENT_SEAT_ADDED:
        if (++seats != 1) { code = 5; break; }
        ei_seat_bind_capabilities(ei_event_get_seat(event), EI_DEVICE_CAP_KEYBOARD, NULL);
        break;
      case EI_EVENT_DEVICE_RESUMED:
        device = ei_event_get_device(event);
        if (!ei_device_has_capability(device, EI_DEVICE_CAP_KEYBOARD)) break;
        if (keyboard || !connected) { code = 5; break; }
        keyboard = ei_device_ref(device);
        ei_device_start_emulating(keyboard, 1);
        started = ready = true;
        puts("{\"eiReady\":true,\"keyboardResumed\":true}");
        break;
      case EI_EVENT_DEVICE_PAUSED:
      case EI_EVENT_DEVICE_REMOVED:
        if (keyboard == ei_event_get_device(event)) { ready = false; code = 5; }
        break;
      case EI_EVENT_SEAT_REMOVED:
        code = 5;
        break;
      default:
        break;
      }
      ei_event_unref(event);
      if (code) break;
    }
    if (code) break;
    struct pollfd fds[2] = {{ei_get_fd(context), POLLIN, 0}, {STDIN_FILENO, POLLIN, 0}};
    int polled = poll(fds, 2, 100);
    if (polled < 0) { if (errno == EINTR) continue; code = 6; break; }
    if (fds[0].revents & (POLLERR | POLLNVAL)) { code = 6; break; }
    if (fds[1].revents & (POLLIN | POLLHUP)) {
      char ch;
      ssize_t n = read(STDIN_FILENO, &ch, 1);
      if (n == 0) { code = 7; break; }
      if (n < 0) { if (errno == EINTR) continue; code = 7; break; }
      if (used + 1 >= sizeof(command)) { code = 7; break; }
      if (ch != '\n') { command[used++] = ch; continue; }
      command[used] = '\0'; used = 0;
      if (!strcmp(command, "stop")) { stop = true; break; }
      if (!ready || sequence >= 6 || command[0] != (char)('0' + sequence) || command[1]) {
        code = 7; break;
      }
      unsigned int k = sequence / 2;
      pressed[k] = sequence % 2 == 0;
      ei_device_keyboard_key(keyboard, keys[k], pressed[k]);
      ei_device_frame(keyboard, ei_now(context));
      ei_dispatch(context);
      printf("{\"edgeSent\":%u}\n", sequence++);
    }
  }
  if (cancelled && !code) code = 8;
  if (keyboard && started && ready) {
    for (unsigned int k = 0; k < 3; k++)
      if (pressed[k]) ei_device_keyboard_key(keyboard, keys[k], false);
    ei_device_frame(keyboard, ei_now(context));
    ei_device_stop_emulating(keyboard);
    ei_dispatch(context);
  }
  if (keyboard) ei_device_unref(keyboard);
  ei_unref(context);
  printf("{\"eiStopped\":true,\"allSixEdgesSent\":%s,\"exitCode\":%d}\n", sequence == 6 ? "true" : "false", code);
  return code;
}
