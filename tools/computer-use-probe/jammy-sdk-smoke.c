/* Native private SDK smoke only: no compositor, desktop or portal access. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <libei.h>
#include <link.h>
#include <limits.h>
#include <pipewire/pipewire.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct module_check {
    const char *prefix;
    unsigned int required;
    int failed;
};

static int inspect_module(struct dl_phdr_info *info, size_t size, void *data)
{
    (void)size;
    struct module_check *check = data;
    const char *name = strrchr(info->dlpi_name, '/');
    name = name ? name + 1 : info->dlpi_name;
    if (strncmp(name, "libpipewire-module-", 19) && strncmp(name, "libspa-", 7))
        return 0;
    char actual[PATH_MAX];
    if (!realpath(info->dlpi_name, actual) ||
        strncmp(actual, check->prefix, strlen(check->prefix)) ||
        actual[strlen(check->prefix)] != '/') {
        fprintf(stderr, "SDK module loaded outside private prefix: %s\n", info->dlpi_name);
        check->failed = 1;
        return 0;
    }
    printf("Owned module=%s\n", actual);
    if (!strcmp(name, "libspa-support.so"))
        check->required |= 1;
    if (!strcmp(name, "libpipewire-module-protocol-native.so"))
        check->required |= 2;
    if (!strcmp(name, "libpipewire-module-client-node.so"))
        check->required |= 4;
    return 0;
}

int main(int argc, char **argv)
{
    if (argc != 3) {
        fputs("Usage: sdk-smoke /canonical/private/prefix expected-version\n", stderr);
        return 2;
    }
    Dl_info identity = {0};
    if (!dladdr((void *)pw_get_library_version, &identity) || !identity.dli_fname)
        return 2;
    char actual[PATH_MAX];
    if (!realpath(identity.dli_fname, actual) ||
        strncmp(actual, argv[1], strlen(argv[1])) || actual[strlen(argv[1])] != '/' ||
        strcmp(pw_get_library_version(), argv[2])) {
        fputs("PipeWire version or private library identity mismatch\n", stderr);
        return 2;
    }
    printf("PipeWire version=%s loaded=%s\n", pw_get_library_version(), identity.dli_fname);
    pw_init(NULL, NULL);
    struct pw_main_loop *loop = pw_main_loop_new(NULL);
    if (!loop)
        return 3;
    struct pw_context *context = pw_context_new(pw_main_loop_get_loop(loop), NULL, 0);
    if (!context) {
        pw_main_loop_destroy(loop);
        return 4;
    }
    struct ei *ei = ei_new_sender(NULL);
    if (!ei) {
        pw_context_destroy(context);
        pw_main_loop_destroy(loop);
        return 5;
    }
    struct module_check check = { .prefix = argv[1], .required = 0, .failed = 0 };
    dl_iterate_phdr(inspect_module, &check);
    ei_unref(ei);
    pw_context_destroy(context);
    pw_main_loop_destroy(loop);
    pw_deinit();
    if (check.failed || check.required != 7) {
        fprintf(stderr, "Private mandatory module mapping incomplete: %u\n", check.required);
        return 6;
    }
    puts("Native SDK contexts created and retired; no remote connection attempted.");
    return 0;
}
