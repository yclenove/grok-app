/* Real private libeis protocol peer. It never injects into the user's desktop. */
#include <libeis.h>
#include <poll.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static struct eis_client *client;
static struct eis_seat *seat;
static struct eis_device *keyboard, *pointer, *relative, *duplicate;
static const char *mapping;
static struct eis_device *make_pointer(const char *name) {
    struct eis_device *d = eis_seat_new_device(seat);
    eis_device_configure_name(d, name);
    eis_device_configure_type(d, EIS_DEVICE_TYPE_VIRTUAL);
    eis_device_configure_capability(d, EIS_DEVICE_CAP_POINTER_ABSOLUTE);
    eis_device_configure_capability(d, EIS_DEVICE_CAP_BUTTON);
    eis_device_configure_capability(d, EIS_DEVICE_CAP_SCROLL);
    struct eis_region *r = eis_device_new_region(d);
    eis_region_set_offset(r, 100, 200);
    eis_region_set_size(r, 800, 600);
    eis_region_set_physical_scale(r, 2.0);
    eis_region_set_mapping_id(r, mapping);
    eis_region_add(r);
    eis_region_unref(r);
    eis_device_add(d);
    eis_device_resume(d);
    return d;
}
static void bind_devices(struct eis_event *event) {
    if (keyboard) return;
    enum eis_device_capability caps[] = { EIS_DEVICE_CAP_KEYBOARD,
        EIS_DEVICE_CAP_POINTER, EIS_DEVICE_CAP_POINTER_ABSOLUTE,
        EIS_DEVICE_CAP_BUTTON, EIS_DEVICE_CAP_SCROLL };
    for (unsigned i = 0; i < sizeof(caps)/sizeof(caps[0]); i++) {
        if (!eis_event_seat_has_capability(event, caps[i])) {
            fprintf(stderr, "required capability not bound\n"); exit(2);
        }
    }
    keyboard = eis_seat_new_device(seat);
    eis_device_configure_name(keyboard, "owned-keyboard");
    eis_device_configure_type(keyboard, EIS_DEVICE_TYPE_VIRTUAL);
    eis_device_configure_capability(keyboard, EIS_DEVICE_CAP_KEYBOARD);
    eis_device_add(keyboard);
    eis_device_resume(keyboard);
    pointer = make_pointer("owned-absolute");
    relative = eis_seat_new_device(seat);
    eis_device_configure_name(relative, "owned-relative");
    eis_device_configure_type(relative, strcmp(mapping, "physical-relative") == 0
        ? EIS_DEVICE_TYPE_PHYSICAL : EIS_DEVICE_TYPE_VIRTUAL);
    if (strcmp(mapping, "physical-relative") == 0)
        eis_device_configure_size(relative, 100, 100);
    eis_device_configure_capability(relative, EIS_DEVICE_CAP_POINTER);
    eis_device_add(relative);
    eis_device_resume(relative);
    puts("{\"event\":\"bound\"}");
}
static void event(struct eis_event *e) {
    enum eis_event_type type = eis_event_get_type(e);
    switch (type) {
    case EIS_EVENT_CLIENT_CONNECT: {
        struct eis_client *c = eis_event_get_client(e);
        if (client || !eis_client_is_sender(c)) { eis_client_disconnect(c); return; }
        client = eis_client_ref(c);
        eis_client_connect(c);
        seat = eis_client_new_seat(c, "owned-seat");
        eis_seat_configure_capability(seat, EIS_DEVICE_CAP_KEYBOARD);
        eis_seat_configure_capability(seat, EIS_DEVICE_CAP_POINTER);
        eis_seat_configure_capability(seat, EIS_DEVICE_CAP_POINTER_ABSOLUTE);
        eis_seat_configure_capability(seat, EIS_DEVICE_CAP_BUTTON);
        eis_seat_configure_capability(seat, EIS_DEVICE_CAP_SCROLL);
        eis_seat_add(seat);
        puts("{\"event\":\"connect\"}");
        break;
    }
    case EIS_EVENT_CLIENT_DISCONNECT: puts("{\"event\":\"disconnect\"}"); break;
    case EIS_EVENT_SEAT_BIND: bind_devices(e); break;
    case EIS_EVENT_DEVICE_CLOSED: eis_device_remove(eis_event_get_device(e)); break;
    case EIS_EVENT_KEYBOARD_KEY:
        printf("{\"event\":\"key\",\"code\":%u,\"pressed\":%s}\n", eis_event_keyboard_get_key(e), eis_event_keyboard_get_key_is_press(e) ? "true" : "false"); break;
    case EIS_EVENT_BUTTON_BUTTON:
        printf("{\"event\":\"button\",\"code\":%u,\"pressed\":%s}\n", eis_event_button_get_button(e), eis_event_button_get_is_press(e) ? "true" : "false"); break;
    case EIS_EVENT_POINTER_MOTION_ABSOLUTE:
        printf("{\"event\":\"absolute\",\"x\":%.8f,\"y\":%.8f}\n", eis_event_pointer_get_absolute_x(e), eis_event_pointer_get_absolute_y(e)); break;
    case EIS_EVENT_POINTER_MOTION:
        printf("{\"event\":\"relative\",\"x\":%.8f,\"y\":%.8f}\n", eis_event_pointer_get_dx(e), eis_event_pointer_get_dy(e)); break;
    case EIS_EVENT_SCROLL_DELTA:
        printf("{\"event\":\"scroll\",\"x\":%.8f,\"y\":%.8f}\n", eis_event_scroll_get_dx(e), eis_event_scroll_get_dy(e)); break;
    case EIS_EVENT_SCROLL_DISCRETE:
        printf("{\"event\":\"discrete\",\"x\":%d,\"y\":%d}\n", eis_event_scroll_get_discrete_dx(e), eis_event_scroll_get_discrete_dy(e)); break;
    case EIS_EVENT_SCROLL_CANCEL: puts("{\"event\":\"scroll-cancel\"}"); break;
    case EIS_EVENT_DEVICE_START_EMULATING:
        printf("{\"event\":\"start\",\"sequence\":%u}\n", eis_event_emulating_get_sequence(e)); break;
    case EIS_EVENT_DEVICE_STOP_EMULATING: puts("{\"event\":\"stop\"}"); break;
    case EIS_EVENT_FRAME:
        printf("{\"event\":\"frame\",\"time\":%llu}\n", (unsigned long long)eis_event_get_time(e)); break;
    default: break;
    }
}
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    mapping = argv[2];
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct eis *ctx = eis_new(NULL);
    if (!ctx || eis_setup_backend_socket(ctx, argv[1]) != 0) return 2;
    puts("{\"event\":\"ready\"}");
    bool running = true;
    while (running) {
        struct pollfd fds[] = {{eis_get_fd(ctx), POLLIN, 0}, {STDIN_FILENO, POLLIN, 0}};
        if (poll(fds, 2, 50) < 0) return 3;
        if (fds[1].revents) {
            char c;
            if (read(STDIN_FILENO, &c, 1) != 1) break;
            switch(c) {
            case 'p': if (keyboard) eis_device_pause(keyboard); puts("{\"event\":\"paused\"}"); break;
            case 'r': if (keyboard) eis_device_resume(keyboard); puts("{\"event\":\"resumed\"}"); break;
            case 'a': if (pointer) eis_device_remove(pointer); puts("{\"event\":\"removed\"}"); break;
            case 'k': if (keyboard) eis_device_remove(keyboard); puts("{\"event\":\"keyboard-removed\"}"); break;
            case 'd': if (seat && !duplicate) duplicate = make_pointer("duplicate-absolute"); puts("{\"event\":\"duplicated\"}"); break;
            case 'x': if (client) eis_client_disconnect(client); break;
            case 'q': running = false; break;
            default: break;
            }
        }
        eis_dispatch(ctx);
        struct eis_event *e;
        while ((e = eis_get_event(ctx))) { event(e); eis_event_unref(e); }
    }
    if (duplicate) eis_device_unref(duplicate);
    if (relative) eis_device_unref(relative);
    if (pointer) eis_device_unref(pointer);
    if (keyboard) eis_device_unref(keyboard);
    if (seat) eis_seat_unref(seat);
    if (client) eis_client_unref(client);
    eis_unref(ctx);
    return 0;
}
