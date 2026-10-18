/* macOS facts probe for OMG (work package R03). Read-only except for a private temporary tree.
 *
 * Prints what the macOS SDK and kernel actually do, so the native adapter is not built on a guess:
 *   - which no-follow / beneath open flags exist and how they behave;
 *   - the stat fields the scanner would use (birth time, flags, blocks, links);
 *   - statfs filesystem names and mount flags for the paths given on the command line;
 *   - whether getattrlistbulk works on a directory descriptor.
 *
 * Usage: probe [path ...]      (each path gets a filesystem report)
 */
#define _DARWIN_C_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __linux__
#include <sys/vfs.h>
#else
#include <sys/mount.h>
#endif
#include <sys/param.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#ifdef __APPLE__
#include <sys/attr.h>
#endif

#define SHOW(name)                                    \
    printf("  %-20s = 0x%x\n", #name, (unsigned)(name))
#define MISSING(name) printf("  %-20s = MISSING in this SDK\n", #name)

static void die(const char *what) {
    fprintf(stderr, "probe harness error: %s: %s\n", what, strerror(errno));
    exit(2);
}

static void try_open(int dirfd, const char *label, const char *path, int flags) {
    int fd = openat(dirfd, path, flags);
    if (fd >= 0) {
        printf("  %-52s OK\n", label);
        close(fd);
    } else {
        printf("  %-52s errno %d (%s)\n", label, errno, strerror(errno));
    }
}

static void write_file(const char *path, size_t size) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) die(path);
    if (size && ftruncate(fd, (off_t)size) != 0) die("ftruncate");
    close(fd);
}

static void flags_section(void) {
    printf("[macros]\n");
#ifdef O_NOFOLLOW_ANY
    SHOW(O_NOFOLLOW_ANY);
#else
    MISSING(O_NOFOLLOW_ANY);
#endif
#ifdef O_RESOLVE_BENEATH
    SHOW(O_RESOLVE_BENEATH);
#else
    MISSING(O_RESOLVE_BENEATH);
#endif
#ifdef O_NOFOLLOW
    SHOW(O_NOFOLLOW);
#endif
#ifdef O_DIRECTORY
    SHOW(O_DIRECTORY);
#endif
#ifdef O_CLOEXEC
    SHOW(O_CLOEXEC);
#endif
#ifdef AT_SYMLINK_NOFOLLOW
    SHOW(AT_SYMLINK_NOFOLLOW);
#endif
#ifdef AT_RESOLVE_BENEATH
    SHOW(AT_RESOLVE_BENEATH);
#else
    MISSING(AT_RESOLVE_BENEATH);
#endif
#ifdef UF_COMPRESSED
    SHOW(UF_COMPRESSED);
#else
    MISSING(UF_COMPRESSED);
#endif
#ifdef SF_DATALESS
    SHOW(SF_DATALESS);
#else
    MISSING(SF_DATALESS);
#endif
#ifdef SF_RESTRICTED
    SHOW(SF_RESTRICTED);
#else
    MISSING(SF_RESTRICTED);
#endif
#ifdef UF_DATAVAULT
    SHOW(UF_DATAVAULT);
#else
    MISSING(UF_DATAVAULT);
#endif
#ifdef ENOTCAPABLE
    SHOW(ENOTCAPABLE);
#else
    MISSING(ENOTCAPABLE);
#endif
#ifdef MNT_LOCAL
    SHOW(MNT_LOCAL);
#endif
#ifdef MNT_DONTBROWSE
    SHOW(MNT_DONTBROWSE);
#endif
#ifdef MNT_ROOTFS
    SHOW(MNT_ROOTFS);
#endif
}

static void open_behaviour(const char *parent) {
    char root[1024], outside[1024], p[1100], q[1100];
    snprintf(root, sizeof root, "%s/root", parent);
    snprintf(outside, sizeof outside, "%s/outside", parent);
    if (mkdir(root, 0700) || mkdir(outside, 0700)) die("mkdir");
    snprintf(p, sizeof p, "%s/a", root);
    if (mkdir(p, 0700)) die("mkdir a");
    snprintf(p, sizeof p, "%s/a/b", root);
    if (mkdir(p, 0700)) die("mkdir a/b");
    snprintf(p, sizeof p, "%s/a/b/file", root);
    write_file(p, 0);
    snprintf(p, sizeof p, "%s/link", root); /* root/link -> a (stays beneath) */
    if (symlink("a", p)) die("symlink link");
    snprintf(p, sizeof p, "%s/a/escape", root); /* root/a/escape -> ../../outside (leaves root) */
    if (symlink("../../outside", p)) die("symlink escape");
    snprintf(p, sizeof p, "%s/outside/inner", parent);
    if (mkdir(p, 0700)) die("mkdir inner");

    int rootfd = open(root, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (rootfd < 0) die("open root");
    int base = O_RDONLY | O_DIRECTORY | O_CLOEXEC;

    printf("[open behaviour] directory fd opened with O_DIRECTORY; tree: a/b/file, link->a, a/escape->../../outside\n");
    try_open(rootfd, "a/b (plain)", "a/b", base);
    try_open(rootfd, "a/b + O_NOFOLLOW", "a/b", base | O_NOFOLLOW);
    try_open(rootfd, "link/b + O_NOFOLLOW (mid-path symlink)", "link/b", base | O_NOFOLLOW);
    try_open(rootfd, "link (final symlink) + O_NOFOLLOW", "link", base | O_NOFOLLOW);
    try_open(rootfd, "a/escape/inner + O_NOFOLLOW", "a/escape/inner", base | O_NOFOLLOW);
#ifdef O_NOFOLLOW_ANY
    try_open(rootfd, "a/b + O_NOFOLLOW_ANY", "a/b", base | O_NOFOLLOW_ANY);
    try_open(rootfd, "link/b + O_NOFOLLOW_ANY (mid-path symlink)", "link/b", base | O_NOFOLLOW_ANY);
    try_open(rootfd, "link + O_NOFOLLOW_ANY (final symlink)", "link", base | O_NOFOLLOW_ANY);
    try_open(rootfd, "a/escape/inner + O_NOFOLLOW_ANY", "a/escape/inner", base | O_NOFOLLOW_ANY);
#endif
#ifdef O_RESOLVE_BENEATH
    try_open(rootfd, "a/b + O_RESOLVE_BENEATH", "a/b", base | O_RESOLVE_BENEATH);
    try_open(rootfd, "link/b + O_RESOLVE_BENEATH (symlink stays inside)", "link/b", base | O_RESOLVE_BENEATH);
    try_open(rootfd, "a/escape/inner + O_RESOLVE_BENEATH (leaves root)", "a/escape/inner", base | O_RESOLVE_BENEATH);
    try_open(rootfd, "../outside + O_RESOLVE_BENEATH (dotdot)", "../outside", base | O_RESOLVE_BENEATH);
    snprintf(q, sizeof q, "%s", outside);
    try_open(rootfd, "absolute path + O_RESOLVE_BENEATH", q, base | O_RESOLVE_BENEATH);
    try_open(rootfd, "a/../a/b + O_RESOLVE_BENEATH (dotdot inside)", "a/../a/b", base | O_RESOLVE_BENEATH);
#endif
#if defined(O_NOFOLLOW_ANY) && defined(O_RESOLVE_BENEATH)
    try_open(rootfd, "a/b + both", "a/b", base | O_NOFOLLOW_ANY | O_RESOLVE_BENEATH);
    try_open(rootfd, "link/b + both", "link/b", base | O_NOFOLLOW_ANY | O_RESOLVE_BENEATH);
    try_open(rootfd, "a/escape/inner + both", "a/escape/inner", base | O_NOFOLLOW_ANY | O_RESOLVE_BENEATH);
#endif

    printf("[stat] fstatat with AT_SYMLINK_NOFOLLOW\n");
    struct stat st;
    if (fstatat(rootfd, "link", &st, AT_SYMLINK_NOFOLLOW) != 0) die("fstatat link");
    printf("  link: S_ISLNK=%d\n", S_ISLNK(st.st_mode) ? 1 : 0);

    /* A sparse file, a hard link and a regular file give the scanner's fields real values. */
    snprintf(p, sizeof p, "%s/a/sparse", root);
    write_file(p, 10u * 1024 * 1024);
    snprintf(q, sizeof q, "%s/a/sparse-link", root);
    if (link(p, q)) die("link");
    if (fstatat(rootfd, "a/sparse", &st, AT_SYMLINK_NOFOLLOW) != 0) die("fstatat sparse");
    printf("  a/sparse: size=%lld blocks=%lld (x512=%lld) nlink=%u dev=%u ino=%llu\n",
           (long long)st.st_size, (long long)st.st_blocks, (long long)st.st_blocks * 512,
           (unsigned)st.st_nlink, (unsigned)st.st_dev, (unsigned long long)st.st_ino);
#ifdef __APPLE__
    printf("  a/sparse: st_flags=0x%x\n", (unsigned)st.st_flags);
    printf("  a/sparse: birthtime=%lld.%09ld mtime=%lld.%09ld ctime=%lld.%09ld\n",
           (long long)st.st_birthtimespec.tv_sec, (long)st.st_birthtimespec.tv_nsec,
           (long long)st.st_mtimespec.tv_sec, (long)st.st_mtimespec.tv_nsec,
           (long long)st.st_ctimespec.tv_sec, (long)st.st_ctimespec.tv_nsec);
#endif
    if (fstatat(rootfd, "a", &st, AT_SYMLINK_NOFOLLOW) != 0) die("fstatat a");
    printf("  a (directory): dev=%u ino=%llu nlink=%u\n", (unsigned)st.st_dev,
           (unsigned long long)st.st_ino, (unsigned)st.st_nlink);

#ifdef __APPLE__
    printf("[getattrlistbulk] on directory fd for root/a\n");
    int afd = openat(rootfd, "a", base);
    if (afd < 0) die("open a");
    struct attrlist al;
    memset(&al, 0, sizeof al);
    al.bitmapcount = ATTR_BIT_MAP_COUNT;
    al.commonattr = ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_NAME | ATTR_CMN_OBJTYPE;
    char buf[16384];
    int n = getattrlistbulk(afd, &al, buf, sizeof buf, 0);
    if (n >= 0)
        printf("  returned %d entries in the first call (a holds b, escape, sparse, sparse-link)\n", n);
    else
        printf("  failed: errno %d (%s)\n", errno, strerror(errno));
    close(afd);
#endif
    close(rootfd);
}

static void fs_report(const char *path) {
    printf("[filesystem] %s\n", path);
#ifdef O_NOFOLLOW_ANY
    /* The scanner opens its root by absolute path with this flag, so the usual locations matter. */
    int rfd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW_ANY);
    if (rfd >= 0) {
        printf("  open as scan root with O_NOFOLLOW_ANY: OK\n");
        close(rfd);
    } else {
        printf("  open as scan root with O_NOFOLLOW_ANY: errno %d (%s)\n", errno, strerror(errno));
    }
#endif
    struct statfs sf;
    if (statfs(path, &sf) != 0) {
        printf("  statfs failed: errno %d (%s)\n", errno, strerror(errno));
        return;
    }
#ifdef __APPLE__
    printf("  type=%s from=%s on=%s\n", sf.f_fstypename, sf.f_mntfromname, sf.f_mntonname);
    printf("  mount flags=0x%x (MNT_LOCAL=%d MNT_DONTBROWSE=%d MNT_ROOTFS=%d RDONLY=%d)\n",
           (unsigned)sf.f_flags,
#ifdef MNT_LOCAL
           (sf.f_flags & MNT_LOCAL) ? 1 : 0,
#else
           -1,
#endif
#ifdef MNT_DONTBROWSE
           (sf.f_flags & MNT_DONTBROWSE) ? 1 : 0,
#else
           -1,
#endif
#ifdef MNT_ROOTFS
           (sf.f_flags & MNT_ROOTFS) ? 1 : 0,
#else
           -1,
#endif
           (sf.f_flags & MNT_RDONLY) ? 1 : 0);
#endif
    printf("  bsize=%u blocks=%llu bfree=%llu bavail=%llu files=%llu ffree=%llu\n",
           (unsigned)sf.f_bsize, (unsigned long long)sf.f_blocks, (unsigned long long)sf.f_bfree,
           (unsigned long long)sf.f_bavail, (unsigned long long)sf.f_files,
           (unsigned long long)sf.f_ffree);
    struct stat st, parent;
    char up[2048];
    snprintf(up, sizeof up, "%s/..", path);
    if (stat(path, &st) == 0 && stat(up, &parent) == 0)
        printf("  st_dev=%u parent st_dev=%u (%s)\n", (unsigned)st.st_dev, (unsigned)parent.st_dev,
               st.st_dev != parent.st_dev ? "mount boundary" : "same device");
}

int main(int argc, char **argv) {
    const char *tmp = getenv("TMPDIR");
    char template[1024];
    snprintf(template, sizeof template, "%s/omg-probe-XXXXXX", tmp && *tmp ? tmp : "/tmp");
    char *parent = mkdtemp(template);
    if (!parent) die("mkdtemp");

    printf("OMG macOS probe\n");
    flags_section();
    open_behaviour(parent);
    fs_report(parent);
    for (int i = 1; i < argc; i++) fs_report(argv[i]);

    /* Best-effort cleanup of the private tree. */
    char cmd[1200];
    snprintf(cmd, sizeof cmd, "rm -rf '%s'", parent);
    if (system(cmd) != 0) fprintf(stderr, "note: could not remove %s\n", parent);
    return 0;
}
