/* Observe only the exact App on this fixture's private X server. */
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <gdk/gdk.h>
#include <gdk/gdkx.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static Window selected = None;
static unsigned matches = 0;

static void find_window(Display *display, Window window, unsigned long pid, unsigned depth) {
    if (depth > 8) return;
    Atom actual;
    int format;
    unsigned long count, remaining;
    unsigned char *data = NULL;
    Atom property = XInternAtom(display, "_NET_WM_PID", False);
    if (XGetWindowProperty(display, window, property, 0, 1, False, XA_CARDINAL,
                           &actual, &format, &count, &remaining, &data) == Success) {
        if (actual == XA_CARDINAL && format == 32 && count == 1 && data &&
            *(unsigned long *)data == pid) {
            XWindowAttributes attributes;
            char *title = NULL;
            if (XGetWindowAttributes(display, window, &attributes) &&
                attributes.map_state == IsViewable && attributes.width >= 800 &&
                attributes.height >= 500 && XFetchName(display, window, &title) &&
                title && strcmp(title, "Grok") == 0) {
                selected = window;
                ++matches;
            }
            if (title) XFree(title);
        }
        if (data) XFree(data);
    }
    Window root, parent, *children = NULL;
    unsigned children_count;
    if (XQueryTree(display, window, &root, &parent, &children, &children_count)) {
        for (unsigned i = 0; i < children_count; ++i)
            find_window(display, children[i], pid, depth + 1);
        if (children) XFree(children);
    }
}

int main(int argc, char **argv) {
    if (argc != 4 || (strcmp(argv[1], "capture") && strcmp(argv[1], "close"))) return 2;
    char *end = NULL;
    unsigned long pid = strtoul(argv[2], &end, 10);
    if (!pid || !end || *end) return 2;
    if (!gdk_init_check(NULL, NULL)) return 3;
    GdkDisplay *gdk_display = gdk_display_get_default();
    if (!GDK_IS_X11_DISPLAY(gdk_display)) return 3;
    Display *display = gdk_x11_display_get_xdisplay(gdk_display);
    find_window(display, DefaultRootWindow(display), pid, 0);
    if (matches != 1) {
        fprintf(stderr, "Expected one mapped exact-PID Grok window, found %u\n", matches);
        return 4;
    }
    if (strcmp(argv[1], "capture") == 0) {
        XWindowAttributes attributes;
        if (!XGetWindowAttributes(display, selected, &attributes)) return 5;
        GdkWindow *window = gdk_x11_window_foreign_new_for_display(gdk_display, selected);
        if (!window) return 5;
        GdkPixbuf *pixels = gdk_pixbuf_get_from_window(window, 0, 0, attributes.width, attributes.height);
        GError *error = NULL;
        gboolean saved = pixels && gdk_pixbuf_save(pixels, argv[3], "png", &error, NULL);
        if (error) { fprintf(stderr, "%s\n", error->message); g_error_free(error); }
        if (pixels) g_object_unref(pixels);
        g_object_unref(window);
        if (!saved) return 5;
        printf("Captured exact App window %lu (%dx%d)\n", selected, attributes.width, attributes.height);
    } else {
        XEvent event = {0};
        event.xclient.type = ClientMessage;
        event.xclient.window = selected;
        event.xclient.message_type = XInternAtom(display, "WM_PROTOCOLS", False);
        event.xclient.format = 32;
        event.xclient.data.l[0] = XInternAtom(display, "WM_DELETE_WINDOW", False);
        event.xclient.data.l[1] = CurrentTime;
        if (!XSendEvent(display, selected, False, NoEventMask, &event)) return 6;
        XSync(display, False);
        printf("Sent one normal WM_DELETE_WINDOW to exact App window %lu\n", selected);
    }
    return 0;
}
