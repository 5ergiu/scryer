import {
  LazyCodeEditor,
  type CodeEditorDiagnostic,
  type CodeEditorProps,
} from "@/components/common/lazy-code-editor";

import { useTranslate } from "@/lib/context/translate-context";
import type { RegoFamily } from "@/lib/utils/rego-assistance";

export type RegoEditorDiagnostic = CodeEditorDiagnostic;

export type LazyRegoEditorProps = Omit<CodeEditorProps, "language" | "regoFamily" | "regoTranslate"> & { ruleFamily: RegoFamily };

export function LazyRegoEditor({ ruleFamily, ...props }: LazyRegoEditorProps) {
  const t = useTranslate();
  return <LazyCodeEditor {...props} language="rego" regoFamily={ruleFamily} regoTranslate={t} />;
}
