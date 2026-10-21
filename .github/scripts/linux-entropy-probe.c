#define _GNU_SOURCE
#include <errno.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

// Supplied from the built archive, including aws-lc-sys's versioned prefix.
void ENTROPY_FUNCTION(uint8_t *, size_t);

static int run_case(int missing_syscall) {
    fflush(NULL);
    pid_t child = fork();
    if (child < 0) { perror("fork"); return 1; }
    if (child == 0) {
        struct rlimit core = {0, 0};
        if (setrlimit(RLIMIT_CORE, &core)) _exit(10);
        alarm(15);
        if (missing_syscall) {
            // Restrict this child only; model kernels predating getrandom.
            struct sock_filter rules[] = {
                BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
                BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_getrandom, 0, 1),
                BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | ENOSYS),
                BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW)
            };
            struct sock_fprog program = {4, rules};
            if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) ||
                prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program)) {
                perror("seccomp");
                _exit(11);
            }
        }
        uint8_t bytes[32];
        errno = 0;
        long result = syscall(SYS_getrandom, bytes, sizeof bytes, 0);
        if (missing_syscall && !(result == -1 && errno == ENOSYS)) _exit(12);
        if (!missing_syscall && result != (long)sizeof bytes) _exit(13);
        ENTROPY_FUNCTION(bytes, sizeof bytes);
        _exit(0);
    }
    int status;
    pid_t waited;
    do { waited = waitpid(child, &status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != child) { perror("waitpid"); return 1; }
    int passed = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    printf("entropy/%s: %s exit=%d signal=%d\n",
           missing_syscall ? "ENOSYS" : "native", passed ? "PASS" : "FAIL",
           WIFEXITED(status) ? WEXITSTATUS(status) : -1,
           WIFSIGNALED(status) ? WTERMSIG(status) : 0);
    return !passed;
}

int main(void) {
    int failures = run_case(0);
    failures += run_case(1);
    return failures ? 1 : 0;
}
