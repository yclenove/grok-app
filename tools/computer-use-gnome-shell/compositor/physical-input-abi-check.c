/* SPDX-License-Identifier: GPL-2.0-or-later */
/* Inspect the actual built native-seat class without constructing a seat,
 * opening devices, starting a compositor or loading it into GNOME Shell.
 * This proves only GObject ABI metadata, NOT live GI delivery or IME behavior.
 */
#include <dlfcn.h>
#include <glib-object.h>

int main (int argc, char **argv)
{
  void *library;
  GType (*get_type) (void);
  GObjectClass *klass;
  GParamSpec *version, *generation;
  GParamSpecUInt *version_spec;
  GParamSpecUInt64 *generation_spec;
  GSignalQuery notify = { 0 };

  if (argc != 2 || !g_path_is_absolute (argv[1]))
    g_error ("Pass one explicit absolute path to the built libmutter library");
  library = dlopen (argv[1], RTLD_NOW | RTLD_LOCAL);
  if (!library)
    g_error ("Cannot load explicit library: %s", dlerror ());
  *(void **) (&get_type) = dlsym (library, "meta_seat_native_get_type");
  if (!get_type)
    g_error ("No native-seat GType in explicit library");
  klass = g_type_class_ref (get_type ());
  g_assert_nonnull (klass);
  g_assert_cmpstr (g_type_name (G_TYPE_FROM_CLASS (klass)), ==, "MetaSeatNative");
  g_assert_cmpstr (g_type_name (g_type_parent (G_TYPE_FROM_CLASS (klass))),
                   ==, "ClutterSeat");
  version = g_object_class_find_property (klass, "grok-physical-input-version");
  generation = g_object_class_find_property (klass, "grok-physical-input-generation");
  g_assert_nonnull (version);
  g_assert_nonnull (generation);
  g_assert_true (G_IS_PARAM_SPEC_UINT (version));
  g_assert_true (G_IS_PARAM_SPEC_UINT64 (generation));
  g_assert_true ((version->flags & G_PARAM_READABLE) != 0);
  g_assert_true ((generation->flags & G_PARAM_READABLE) != 0);
  g_assert_cmpuint (version->flags & (G_PARAM_WRITABLE | G_PARAM_CONSTRUCT |
                                    G_PARAM_CONSTRUCT_ONLY), ==, 0);
  g_assert_cmpuint (generation->flags & (G_PARAM_WRITABLE | G_PARAM_CONSTRUCT |
                                       G_PARAM_CONSTRUCT_ONLY), ==, 0);
  g_assert_true ((generation->flags & G_PARAM_EXPLICIT_NOTIFY) != 0);
  version_spec = G_PARAM_SPEC_UINT (version);
  generation_spec = G_PARAM_SPEC_UINT64 (generation);
  g_assert_cmpuint (version_spec->minimum, ==, 1);
  g_assert_cmpuint (version_spec->maximum, ==, 1);
  g_assert_cmpuint (version_spec->default_value, ==, 1);
  g_assert_cmpuint (generation_spec->minimum, ==, 0);
  g_assert_cmpuint (generation_spec->maximum, ==, G_MAXUINT64);
  g_assert_cmpuint (generation_spec->default_value, ==, 0);
  g_signal_query (g_signal_lookup ("notify", G_TYPE_FROM_CLASS (klass)), &notify);
  g_assert_true ((notify.signal_flags & G_SIGNAL_DETAILED) != 0);
  g_assert_cmpuint (notify.return_type, ==, G_TYPE_NONE);
  g_assert_cmpuint (notify.n_params, ==, 1);
  g_assert_cmpuint (notify.param_types[0] & ~G_SIGNAL_TYPE_STATIC_SCOPE,
                    ==, G_TYPE_PARAM);
  g_print ("{\"nativeType\":\"MetaSeatNative\",\"version\":1,"
           "\"generationType\":\"guint64\",\"readOnly\":true,"
           "\"explicitNotify\":true,\"seatConstructed\":false,"
           "\"liveGiVerified\":false,\"nativeInputVerified\":false}\n");
  g_type_class_unref (klass);
  /* Registered static GTypes reference this DSO until process exit. Do not
   * dlclose it while the process's type registry still contains those types. */
  return 0;
}
