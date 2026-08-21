#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: x11_widget OUTFILE READYFILE\n");
        return 2;
    }
    Display *display = XOpenDisplay(NULL);
    if (!display) {
        fprintf(stderr, "XOpenDisplay failed\n");
        return 1;
    }
    int screen = DefaultScreen(display);
    Window window = XCreateSimpleWindow(
        display,
        RootWindow(display, screen),
        40,
        40,
        480,
        80,
        1,
        BlackPixel(display, screen),
        WhitePixel(display, screen)
    );
    XStoreName(display, window, "echo-inject-target");
    XSelectInput(display, window, KeyPressMask | ExposureMask | FocusChangeMask);
    XMapRaised(display, window);
    XFlush(display);
    usleep(50000);
    XSetInputFocus(display, window, RevertToParent, CurrentTime);
    XFlush(display);

    FILE *ready = fopen(argv[2], "w");
    if (!ready) {
        return 1;
    }
    fprintf(ready, "%lu\n", (unsigned long)window);
    fclose(ready);

    FILE *out = fopen(argv[1], "w");
    if (!out) {
        return 1;
    }
    for (;;) {
        XEvent event;
        XNextEvent(display, &event);
        if (event.type != KeyPress) {
            continue;
        }
        char ch[16];
        KeySym keysym = 0;
        int len = XLookupString(&event.xkey, ch, (int)sizeof(ch) - 1, &keysym, NULL);
        if (keysym == XK_Return) {
            break;
        }
        if (len <= 0) {
            continue;
        }
        fwrite(ch, 1, (size_t)len, out);
        fflush(out);
    }
    fclose(out);
    XDestroyWindow(display, window);
    XCloseDisplay(display);
    return 0;
}
