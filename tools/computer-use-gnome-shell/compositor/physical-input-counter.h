/* SPDX-License-Identifier: GPL-2.0-or-later */
/* Experimental Mutter-side read-only counter, NOT installed by the App.
 * Call record only at native libinput ingress, before ANY event filtering.
 * No event content, device access, input interception or reinjection.
 * init/close/clear and dispatch belong to the compositor main thread;
 * record/read are thread safe. Join the input thread before close/clear.
 */
#pragma once

#include <glib-object.h>

typedef struct
{
  GMutex mutex;
  GMainContext *context;
  GObject *owner; /* Borrowed; each pending source takes its own strong ref. */
  guint signal_id;
  guint64 generation;
  GSource *source; /* Exactly one owned ref, or NULL. */
  gboolean closed;
} MetaPhysicalInputCounter;

typedef struct
{
  MetaPhysicalInputCounter *counter;
  GObject *owner;
  GSource *source; /* Borrowed from the live dispatch source. */
} MetaPhysicalInputDelivery;

static inline void
meta_physical_input_delivery_free (gpointer data)
{
  MetaPhysicalInputDelivery *delivery = data;

  g_object_unref (delivery->owner);
  g_free (delivery);
}

static inline gboolean
meta_physical_input_counter_dispatch (gpointer data)
{
  MetaPhysicalInputDelivery *delivery = data;
  MetaPhysicalInputCounter *counter = delivery->counter;
  GSource *source = NULL;
  gboolean emit = FALSE;

  g_mutex_lock (&counter->mutex);
  if (counter->source == delivery->source)
    {
      source = g_steal_pointer (&counter->source);
      emit = !counter->closed;
    }
  g_mutex_unlock (&counter->mutex);

  g_clear_pointer (&source, g_source_unref);
  if (emit)
    g_signal_emit (delivery->owner, counter->signal_id, 0);
  return G_SOURCE_REMOVE;
}

static inline void
meta_physical_input_counter_init (MetaPhysicalInputCounter *counter,
                                 GObject                  *owner,
                                 GMainContext             *context,
                                 guint                     signal_id)
{
  g_mutex_init (&counter->mutex);
  counter->context = g_main_context_ref (context);
  counter->owner = owner;
  counter->signal_id = signal_id;
  counter->generation = 0;
  counter->source = NULL;
  counter->closed = FALSE;
}

static inline guint64
meta_physical_input_counter_read (MetaPhysicalInputCounter *counter)
{
  guint64 generation;

  g_mutex_lock (&counter->mutex);
  generation = counter->closed ? G_MAXUINT64 : counter->generation;
  g_mutex_unlock (&counter->mutex);
  return generation;
}

static inline void
meta_physical_input_counter_record (MetaPhysicalInputCounter *counter)
{
  g_mutex_lock (&counter->mutex);
  if (!counter->closed)
    {
      /* Saturation is a permanent fault sentinel, never a wrapped grant. */
      if (counter->generation < G_MAXUINT64)
        counter->generation++;
      if (!counter->source)
        {
          MetaPhysicalInputDelivery *delivery;

          counter->source = g_idle_source_new ();
          g_source_set_priority (counter->source, G_PRIORITY_HIGH);
          g_source_set_name (counter->source, "physical-input-generation");
          delivery = g_new0 (MetaPhysicalInputDelivery, 1);
          delivery->counter = counter;
          delivery->owner = g_object_ref (counter->owner);
          delivery->source = counter->source;
          g_source_set_callback (counter->source,
                                 meta_physical_input_counter_dispatch,
                                 delivery, meta_physical_input_delivery_free);
          g_source_attach (counter->source, counter->context);
        }
    }
  g_mutex_unlock (&counter->mutex);
}

static inline void
meta_physical_input_counter_close (MetaPhysicalInputCounter *counter)
{
  GSource *source;

  g_mutex_lock (&counter->mutex);
  counter->closed = TRUE;
  source = g_steal_pointer (&counter->source);
  g_mutex_unlock (&counter->mutex);
  if (source)
    {
      g_source_destroy (source);
      g_source_unref (source);
    }
}

static inline void
meta_physical_input_counter_clear (MetaPhysicalInputCounter *counter)
{
  meta_physical_input_counter_close (counter);
  g_clear_pointer (&counter->context, g_main_context_unref);
  g_mutex_clear (&counter->mutex);
}
