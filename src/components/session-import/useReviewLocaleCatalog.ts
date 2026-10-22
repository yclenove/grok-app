import { useEffect, useState } from "react";
import { isLocaleCatalogReady, loadLocaleCatalog, type Locale } from "@/i18n";

/** The isolated browser entry does not have the normal App locale loader. */
export function useReviewLocaleCatalog(locale: Locale) {
  const [loaded, setLoaded] = useState<Locale | null>(() =>
    isLocaleCatalogReady(locale) ? locale : null,
  );
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let disposed = false;
    setError(false);
    if (isLocaleCatalogReady(locale)) {
      setLoaded(locale);
      return;
    }
    void loadLocaleCatalog(locale).then(
      () => {
        if (!disposed) setLoaded(locale);
      },
      () => {
        if (!disposed) setError(true);
      },
    );
    return () => { disposed = true; };
  }, [locale, attempt]);

  return {
    ready: loaded === locale || isLocaleCatalogReady(locale),
    error,
    retry: () => setAttempt((value) => value + 1),
  };
}
