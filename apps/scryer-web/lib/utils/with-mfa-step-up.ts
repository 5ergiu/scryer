import { isMfaStepUpRequiredError } from "../graphql/error-message.ts";

/** Retry only a rejected MFA challenge, once, with the newly verified session. */
export async function withMfaStepUp<T extends { error?: unknown }>(
  submit: (verifiedToken?: string) => Promise<T>,
  verify: () => Promise<string | null>,
): Promise<T | null> {
  const result = await submit();
  if (!isMfaStepUpRequiredError(result.error)) return result;
  const token = await verify();
  return token ? submit(token) : null;
}
