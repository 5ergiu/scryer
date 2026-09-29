import { regoCodeAt } from "./rego-lexical.ts";

// Match the backend package/import insertion without inventing positions.
export function regoSourceLine(source: string, compiledLine: number): number {
  const lines = source.split("\n");
  const packageIndex = lines.findIndex((line) => line.trim().startsWith("package "));
  const hasImport = lines.some((line) => line.trim() === "import rego.v1");
  let line = compiledLine - (packageIndex < 0 ? 1 : 0);
  if (!hasImport && line > (packageIndex < 0 ? 0 : packageIndex + 1)) line--;
  return line;
}

export type RegoValidationResult = { valid: boolean; errors: string[]; unavailable?: boolean };
export type RegoDiagnostic = { line: number; column: number; message: string };

export function regoDiagnostics(source: string, result: RegoValidationResult | null): RegoDiagnostic[] {
  if (!result || result.valid || result.unavailable) return [];
  return result.errors.flatMap((message) => {
    const location = message.match(/\S+\.rego:(\d+):(\d+)/);
    if (location) {
      const line = regoSourceLine(source, Number(location[1]));
      const column = Number(location[2]);
      if (line > 0 && line <= source.split("\n").length && column > 0) return [{ line, column, message }];
      return [];
    }
    const path = message.match(/(?:Unknown|Unsupported dynamic) rule input path '([^']+)'/)?.[1];
    if (!path) return [];
    const index = source.indexOf(path);
    if (index < 0 || source.indexOf(path, index + 1) >= 0 || !regoCodeAt(source, index)) return [];
    const before = source.slice(0, index);
    return [{ line: before.split("\n").length, column: index - before.lastIndexOf("\n"), message }];
  });
}
