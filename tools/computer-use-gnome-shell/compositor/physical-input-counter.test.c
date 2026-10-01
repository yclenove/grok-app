/* SPDX-License-Identifier: GPL-2.0-or-later */
#include "physical-input-counter.h"

typedef struct { GObject parent; } CounterOwner;
typedef struct { GObjectClass parent; } CounterOwnerClass;
G_DEFINE_TYPE (CounterOwner, counter_owner, G_TYPE_OBJECT)
static guint signal_id;
static void counter_owner_init (CounterOwner *owner) { (void) owner; }
static void counter_owner_class_init (CounterOwnerClass *klass)
{
  signal_id = g_signal_new ("physical-input", G_TYPE_FROM_CLASS (klass),
                            G_SIGNAL_RUN_LAST, 0, NULL, NULL, NULL,
                            G_TYPE_NONE, 0);
}

typedef struct
{
  MetaPhysicalInputCounter counter;
  GMainContext *context;
  GObject *owner;
  GThread *main_thread;
  guint notifications;
  gboolean reenter;
} Harness;

static void on_input (GObject *owner, Harness *h)
{
  g_assert_true (owner == h->owner);
  g_assert_true (g_thread_self () == h->main_thread);
  h->notifications++;
  g_assert_cmpuint (meta_physical_input_counter_read (&h->counter), >, 0);
  if (h->reenter)
    {
      h->reenter = FALSE;
      meta_physical_input_counter_record (&h->counter);
    }
}

static void init (Harness *h)
{
  *h = (Harness) { 0 };
  h->main_thread = g_thread_self ();
  h->context = g_main_context_new ();
  h->owner = g_object_new (counter_owner_get_type (), NULL);
  meta_physical_input_counter_init (&h->counter, h->owner, h->context, signal_id);
  g_signal_connect (h->owner, "physical-input", G_CALLBACK (on_input), h);
}

static void clear (Harness *h)
{
  meta_physical_input_counter_clear (&h->counter);
  g_object_unref (h->owner);
  g_main_context_unref (h->context);
}

static void drain (Harness *h)
{
  guint limit = 10000;
  while (g_main_context_iteration (h->context, FALSE))
    g_assert_cmpuint (--limit, >, 0);
}

static void test_read_before_notify (void)
{
  Harness h; init (&h);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, 0);
  for (guint i = 0; i < 1000; i++)
    meta_physical_input_counter_record (&h.counter);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, 1000);
  g_assert_cmpuint (h.notifications, ==, 0);
  drain (&h);
  g_assert_cmpuint (h.notifications, ==, 1);
  g_assert_null (h.counter.source);
  clear (&h);
}

static gpointer writer (gpointer data)
{
  Harness *h = data;
  for (guint i = 0; i < 25000; i++)
    meta_physical_input_counter_record (&h->counter);
  return NULL;
}

static void test_concurrent_writers (void)
{
  Harness h; init (&h);
  GThread *threads[4];
  for (guint i = 0; i < G_N_ELEMENTS (threads); i++)
    threads[i] = g_thread_new ("input", writer, &h);
  guint64 last = 0;
  while (last < 100000)
    {
      guint64 now = meta_physical_input_counter_read (&h.counter);
      g_assert_cmpuint (now, >=, last);
      last = now;
      g_main_context_iteration (h.context, FALSE);
      g_thread_yield ();
    }
  for (guint i = 0; i < G_N_ELEMENTS (threads); i++)
    g_thread_join (threads[i]);
  drain (&h);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, 100000);
  g_assert_cmpuint (h.notifications, >, 0);
  g_assert_null (h.counter.source);
  clear (&h);
}

static void test_reentrant_delivery (void)
{
  Harness h; init (&h); h.reenter = TRUE;
  meta_physical_input_counter_record (&h.counter);
  drain (&h);
  g_assert_cmpuint (h.notifications, ==, 2);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, 2);
  clear (&h);
}

static void test_close_pending (void)
{
  Harness h; init (&h);
  meta_physical_input_counter_record (&h.counter);
  meta_physical_input_counter_close (&h.counter);
  meta_physical_input_counter_close (&h.counter);
  meta_physical_input_counter_record (&h.counter);
  drain (&h);
  g_assert_cmpuint (h.notifications, ==, 0);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, G_MAXUINT64);
  g_assert_null (h.counter.source);
  clear (&h);
}

static void finalized (gpointer data, GObject *owner)
{
  (void) owner;
  *(gboolean *) data = TRUE;
}

static void test_source_owns_original_object (void)
{
  Harness h; init (&h);
  gboolean gone = FALSE;
  g_object_weak_ref (h.owner, finalized, &gone);
  meta_physical_input_counter_record (&h.counter);
  g_object_unref (h.owner);
  g_assert_false (gone);
  meta_physical_input_counter_close (&h.counter);
  g_assert_true (gone);
  drain (&h);
  g_assert_cmpuint (h.notifications, ==, 0);
  meta_physical_input_counter_clear (&h.counter);
  g_main_context_unref (h.context);
}

static void test_saturates (void)
{
  Harness h; init (&h);
  h.counter.generation = G_MAXUINT64 - 1;
  meta_physical_input_counter_record (&h.counter);
  meta_physical_input_counter_record (&h.counter);
  g_assert_cmpuint (meta_physical_input_counter_read (&h.counter), ==, G_MAXUINT64);
  drain (&h);
  g_assert_cmpuint (h.notifications, ==, 1);
  clear (&h);
}

int main (int argc, char **argv)
{
  g_test_init (&argc, &argv, NULL);
  g_test_add_func ("/counter/read-before-notify-coalesces", test_read_before_notify);
  g_test_add_func ("/counter/concurrent-writers-main-delivery", test_concurrent_writers);
  g_test_add_func ("/counter/reentrant-delivery", test_reentrant_delivery);
  g_test_add_func ("/counter/close-pending-is-sticky", test_close_pending);
  g_test_add_func ("/counter/original-source-ownership", test_source_owns_original_object);
  g_test_add_func ("/counter/saturates-without-wrapping", test_saturates);
  return g_test_run ();
}
