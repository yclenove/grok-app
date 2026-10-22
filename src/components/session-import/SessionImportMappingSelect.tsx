import type { ComponentProps } from "react";
import { Select } from "@/components/Select";
import type { MessageKey, Vars } from "@/i18n";

type Props = Pick<ComponentProps<typeof Select>, "value" | "options" | "onChange" | "disabled"> & {
  sourceLabel: string;
  tr: (key: MessageKey, vars?: Vars) => string;
};

/** Keep the source and visible selection in the name of the real custom Select. */
export function SessionImportMappingSelect({ sourceLabel, tr, ...props }: Props) {
  const selected = props.options.find((option) => option.value === props.value) ?? props.options[0];
  return <Select
    {...props}
    aria-label={tr("sessionImport.mappingTarget", { source: sourceLabel, target: selected?.label ?? props.value })}
  />;
}
