#define _GNU_SOURCE
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

static int control_path(char *path, size_t size, const char *name) {
    const char *root = getenv("HITO_DOWNLOAD_CLEANUP_GATE_ROOT");
    struct stat metadata;
    if (!root || lstat(root, &metadata) != 0 || !S_ISDIR(metadata.st_mode) ||
        metadata.st_uid != geteuid() || (metadata.st_mode & 0777) != 0700)
        return 0;
    int length = snprintf(path, size, "%s/%s", root, name);
    return length > 0 && (size_t)length < size;
}

static int owned_directory(int descriptor) {
    char selected[PATH_MAX];
    char enabled[PATH_MAX];
    if (!control_path(selected, sizeof(selected), "managed-root") ||
        !control_path(enabled, sizeof(enabled), "enabled") || access(enabled, F_OK) != 0)
        return 0;
    int selection = open(selected, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (selection < 0) return 0;
    struct stat selection_metadata;
    int valid = fstat(selection, &selection_metadata) == 0 &&
        S_ISREG(selection_metadata.st_mode) && selection_metadata.st_uid == geteuid() &&
        (selection_metadata.st_mode & 0777) == 0600 && selection_metadata.st_nlink == 1;
    char root[PATH_MAX];
    ssize_t length = valid ? read(selection, root, sizeof(root) - 1) : -1;
    close(selection);
    if (length <= 0 || length >= (ssize_t)sizeof(root) - 1) return 0;
    root[length] = '\0';
    const char *fixtures = getenv("HITO_DOWNLOAD_CLEANUP_FIXTURE_ROOT");
    size_t prefix = fixtures ? strlen(fixtures) : 0;
    if (!prefix || strncmp(root, fixtures, prefix) != 0 || root[prefix] != '/') return 0;
    struct stat actual;
    struct stat expected;
    if (fstat(descriptor, &actual) != 0 || lstat(root, &expected) != 0 ||
        !S_ISDIR(actual.st_mode) || !S_ISDIR(expected.st_mode) ||
        actual.st_uid != geteuid() || (actual.st_mode & 0777) != 0700 ||
        actual.st_dev != expected.st_dev || actual.st_ino != expected.st_ino)
        return 0;
    char link[64];
    char target[PATH_MAX];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", descriptor);
    length = readlink(link, target, sizeof(target) - 1);
    if (length < 0 || length >= (ssize_t)sizeof(target) - 1) return 0;
    target[length] = '\0';
    return strcmp(root, target) == 0;
}

static void marker(const char *name) {
    char path[PATH_MAX];
    if (!control_path(path, sizeof(path), name)) return;
    int descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (descriptor >= 0) close(descriptor);
}

int fsync(int descriptor) {
    int (*original)(int) = dlsym(RTLD_NEXT, "fsync");
    if (!original) { errno = EIO; return -1; }
    int expected = 0;
    if (!owned_directory(descriptor) || !atomic_compare_exchange_strong(&entered, &expected, 1))
        return original(descriptor);
    char release[PATH_MAX];
    char active[PATH_MAX];
    if (!control_path(release, sizeof(release), "release") ||
        !control_path(active, sizeof(active), "active"))
        return original(descriptor);
    marker("active");
    marker("entered");
    struct timespec delay = {0, 10000000};
    while (access(release, F_OK) != 0) nanosleep(&delay, NULL);
    int result = original(descriptor);
    int result_errno = errno;
    unlink(active);
    marker("retired");
    errno = result_errno;
    return result;
}
