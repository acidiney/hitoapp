#define _GNU_SOURCE
#include <ctype.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static _Atomic int entered = 0;

static int uuid_name(const char *value) {
    for (int index = 0; index < 36; index++) {
        if (index == 8 || index == 13 || index == 18 || index == 23) {
            if (value[index] != '-') return 0;
        } else if (!isxdigit((unsigned char)value[index])) {
            return 0;
        }
    }
    return 1;
}

static int owned_part(int descriptor) {
    const char *root = getenv("HITO_SNAPSHOT_MANAGED_ROOT");
    struct stat metadata;
    if (!root || fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_uid != geteuid() || metadata.st_nlink != 1 || (metadata.st_mode & 077) != 0)
        return 0;
    char link[64];
    char path[PATH_MAX];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", descriptor);
    ssize_t length = readlink(link, path, sizeof(path) - 1);
    if (length < 0 || length >= (ssize_t)sizeof(path) - 1) return 0;
    path[length] = '\0';
    size_t root_length = strlen(root);
    if (strncmp(path, root, root_length) != 0 || path[root_length] != '/') return 0;
    const char *name = path + root_length + 1;
    return strlen(name) == 78 && uuid_name(name) && name[36] == '.' &&
        uuid_name(name + 37) && strcmp(name + 73, ".part") == 0;
}

static int control_path(char *path, size_t size, const char *name) {
    const char *root = getenv("HITO_SNAPSHOT_GATE_ROOT");
    if (!root) return 0;
    int length = snprintf(path, size, "%s/%s", root, name);
    return length > 0 && (size_t)length < size;
}

static void marker(const char *name) {
    char path[PATH_MAX];
    if (!control_path(path, sizeof(path), name)) return;
    int descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor >= 0) close(descriptor);
}

ssize_t read(int descriptor, void *buffer, size_t count) {
    ssize_t (*original)(int, void *, size_t) = dlsym(RTLD_NEXT, "read");
    if (!original) { errno = EIO; return -1; }
    int expected = 0;
    if (!owned_part(descriptor) || !atomic_compare_exchange_strong(&entered, &expected, 1))
        return original(descriptor, buffer, count);
    char release[PATH_MAX];
    char active[PATH_MAX];
    if (!control_path(release, sizeof(release), "release") ||
        !control_path(active, sizeof(active), "active"))
        return original(descriptor, buffer, count);
    marker("active");
    marker("entered");
    struct timespec delay = {0, 10000000};
    while (access(release, F_OK) != 0) nanosleep(&delay, NULL);
    ssize_t result = original(descriptor, buffer, count);
    int result_errno = errno;
    unlink(active);
    marker("retired");
    errno = result_errno;
    return result;
}
