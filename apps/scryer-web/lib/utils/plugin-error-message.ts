import type { Translate } from "@/components/root/types";
import {
  graphQlErrorExtension,
  userFacingGraphQlErrorMessage,
} from "../graphql/error-message.ts";

/**
 * A failed plugin call reaches the client as a `PLUGIN_FAILED` error whose
 * `extensions.reason` names why it failed, beside the server's English
 * sentence. The plugin's own sentence, when it said something specific, rides
 * as `extensions.detail`. Each reason listed here is said in the viewer's
 * language, with the plugin's sentence after it; any other failure shows the
 * server's sentence as before.
 */
export const PLUGIN_FAILURE_KEYS: Readonly<Record<string, string>> = {
  PLUGIN_INVALID_CONFIG: "plugins.failure.invalidConfig",
  PLUGIN_AUTH_FAILED: "plugins.failure.authFailed",
  PLUGIN_RATE_LIMITED: "plugins.failure.rateLimited",
  PLUGIN_UPSTREAM_UNAVAILABLE: "plugins.failure.upstreamUnavailable",
  PLUGIN_UNSUPPORTED: "plugins.failure.unsupported",
  PLUGIN_TEMPORARY_FAILURE: "plugins.failure.temporary",
  PLUGIN_PERMANENT_FAILURE: "plugins.failure.permanent",
  PLUGIN_HOST_FAILURE: "plugins.failure.hostFailure",
  PLUGIN_TIMED_OUT: "plugins.failure.timedOut",
  PLUGIN_RESOURCE_LIMIT: "plugins.failure.resourceLimit",
};

const PLUGIN_FAILED = "PLUGIN_FAILED";

/** The translation key for the reason a plugin call failed, if known. */
export function pluginFailureKey(error: unknown): string | null {
  const reason = graphQlErrorExtension(error, PLUGIN_FAILED, "reason");
  return reason !== null && Object.hasOwn(PLUGIN_FAILURE_KEYS, reason)
    ? PLUGIN_FAILURE_KEYS[reason]
    : null;
}

/** The plugin's own sentence about a failed call, when it gave one. */
export function pluginFailureDetail(error: unknown): string | null {
  const detail = graphQlErrorExtension(error, PLUGIN_FAILED, "detail")?.trim();
  return detail ? detail : null;
}

/**
 * What to show for a failed plugin call: the translated reason, followed by
 * the plugin's own sentence when it gave one, when the server named a reason
 * this client knows; otherwise the server's own sentence, and `fallback` when
 * the failure carries no usable sentence.
 */
export function pluginErrorMessage(error: unknown, t: Translate, fallback: string): string {
  const key = pluginFailureKey(error);
  if (!key) {
    return userFacingGraphQlErrorMessage(error, fallback);
  }
  const reason = t(key);
  const detail = pluginFailureDetail(error);
  return detail ? t("plugins.failure.withDetail", { reason, detail }) : reason;
}
