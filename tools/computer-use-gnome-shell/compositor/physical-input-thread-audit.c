/* SPDX-License-Identifier: GPL-2.0-or-later */
/* Owned-VM TEST ONLY: LD_PRELOAD observer of the original GLib thread object.
 * Never wraps the input function, changes scheduling or handles input events.
 * An OS thread name is not a reliable identity; require the exact GThread* from
 * g_thread_try_new and completion of its original g_thread_join instead.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <glib.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static _Atomic (GThread *) original;
static atomic_uint created, joined, violations;
static int receipt_fd = -1;

GThread *g_thread_try_new (const gchar *name, GThreadFunc func,
                         gpointer data, GError **error)
{
  GThread *(*real_new) (const gchar *, GThreadFunc, gpointer, GError **);
  GThread *thread;
  *(void **) (&real_new) = dlsym (RTLD_NEXT, "g_thread_try_new");
  if (!real_new) _exit (110);
  thread = real_new (name, func, data, error);
  if (thread && name && strcmp (name, "Mutter Input Thread") == 0)
    {
      const char *gate = getenv ("GROK_CU_OWNED_VM_COUNTER_ABI");
      const char *path = getenv ("GROK_CU_THREAD_AUDIT");
      const char *prefix = "/home/cuaccept/cu-acceptance/physical-counter-build-20261001/gi-run-";
      if (!gate || strcmp (gate, "812400f8-a6c6-4c38-a735-2b8d0ef8d3e2") ||
          !path || strncmp (path, prefix, strlen (prefix))) _exit (111);
      if (atomic_fetch_add (&created, 1) != 0) atomic_fetch_add (&violations, 1);
      if (atomic_exchange (&original, thread)) atomic_fetch_add (&violations, 1);
      if (receipt_fd < 0)
        receipt_fd = open (path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
      if (receipt_fd < 0) _exit (112);
    }
  return thread;
}

gpointer g_thread_join (GThread *thread)
{
  gpointer (*real_join) (GThread *);
  gpointer result;
  gboolean matched = thread && thread == atomic_load (&original);
  *(void **) (&real_join) = dlsym (RTLD_NEXT, "g_thread_join");
  if (!real_join) _exit (113);
  result = real_join (thread); /* Do not replace or bypass the actual join. */
  if (matched)
    {
      atomic_store (&original, NULL);
      atomic_fetch_add (&joined, 1);
    }
  return result;
}

__attribute__((destructor)) static void finish_audit (void)
{
  if (receipt_fd >= 0)
    {
      char json[256];
      int length = snprintf (json, sizeof json,
        "{\"created\":%u,\"originalJoinCompleted\":%u,\"violations\":%u,"
        "\"originalStillLive\":%s,\"inputFunctionChanged\":false}\n",
        atomic_load (&created), atomic_load (&joined), atomic_load (&violations),
        atomic_load (&original) ? "true" : "false");
      if (length <= 0 || (size_t) length >= sizeof json ||
          write (receipt_fd, json, (size_t) length) != length || fsync (receipt_fd))
        _exit (114);
      close (receipt_fd);
    }
}
