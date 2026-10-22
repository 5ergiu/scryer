import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import en from "../i18n/locales/en.ts";
import {
  PLUGIN_FAILURE_KEYS,
  pluginErrorMessage,
  pluginFailureDetail,
  pluginFailureKey,
} from "./plugin-error-message.ts";

const dictionary = en as Record<string, string>;
const t = (key: string, values?: Record<string, unknown>) => {
  let result = dictionary[key] ?? key;
  for (const [name, value] of Object.entries(values ?? {})) {
    result = result.replaceAll(`{{${name}}}`, String(value ?? ""));
  }
  return result;
};

function failure(message: string, extensions: Record<string, unknown>) {
  return { graphQLErrors: [{ message, extensions }] };
}

test("a named plugin failure is shown in translation, not as the server's sentence", () => {
  const error = failure(
    "indexer connection test failed: indexer indexer_search component: plugin error Temporary: indexer component failed",
    { code: "PLUGIN_FAILED", reason: "PLUGIN_TEMPORARY_FAILURE" },
  );

  assert.equal(pluginFailureKey(error), "plugins.failure.temporary");
  assert.equal(pluginFailureDetail(error), null);
  assert.equal(
    pluginErrorMessage(error, t, "Test failed"),
    "The plugin ran into a temporary problem. Try again in a little while.",
  );
});

test("the plugin's own sentence follows the translated reason", () => {
  const error = failure("download test_connection: plugin error AuthFailed: Login refused", {
    code: "PLUGIN_FAILED",
    reason: "PLUGIN_AUTH_FAILED",
    detail: "Login refused",
  });

  assert.equal(pluginFailureDetail(error), "Login refused");
  assert.equal(
    pluginErrorMessage(error, t, "Test failed"),
    "The service rejected the sign-in details. Check the API key, username, or password. Details: Login refused",
  );
});

test("a reason this client does not know falls back to the server's sentence", () => {
  const error = failure("plugin error Mystery: the dial is set to eleven", {
    code: "PLUGIN_FAILED",
    reason: "PLUGIN_SOMETHING_NEW",
    detail: "the dial is set to eleven",
  });

  assert.equal(pluginFailureKey(error), null);
  assert.equal(
    pluginErrorMessage(error, t, "Test failed"),
    "plugin error Mystery: the dial is set to eleven",
  );
});

test("a reason on another kind of error is not read as a plugin failure", () => {
  const error = failure("validation: The indexer rejected the API key.", {
    code: "VALIDATION_ERROR",
    reason: "PLUGIN_AUTH_FAILED",
  });

  assert.equal(pluginFailureKey(error), null);
  assert.equal(pluginErrorMessage(error, t, "Test failed"), "The indexer rejected the API key.");
});

test("a failure with no sentence at all shows the fallback", () => {
  assert.equal(pluginErrorMessage(new Error(""), t, "Test failed"), "Test failed");
});

test("every reason the server can name has a translation in every locale", () => {
  const reasons = [
    "PLUGIN_INVALID_CONFIG",
    "PLUGIN_AUTH_FAILED",
    "PLUGIN_RATE_LIMITED",
    "PLUGIN_UPSTREAM_UNAVAILABLE",
    "PLUGIN_UNSUPPORTED",
    "PLUGIN_TEMPORARY_FAILURE",
    "PLUGIN_PERMANENT_FAILURE",
    "PLUGIN_HOST_FAILURE",
    "PLUGIN_TIMED_OUT",
    "PLUGIN_RESOURCE_LIMIT",
  ];
  assert.deepEqual(Object.keys(PLUGIN_FAILURE_KEYS).sort(), [...reasons].sort());

  const localesDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "../i18n/locales");
  const keys = [...Object.values(PLUGIN_FAILURE_KEYS), "plugins.failure.withDetail"];
  for (const file of readdirSync(localesDir).filter((name) => name.endsWith(".ts"))) {
    const source = readFileSync(path.join(localesDir, file), "utf8");
    for (const key of keys) {
      assert.ok(source.includes(`"${key}":`), `${file} is missing ${key}`);
    }
    const withDetail = source.match(/"plugins\.failure\.withDetail":\s*"([^"]*)"/)?.[1] ?? "";
    assert.ok(
      withDetail.includes("{{reason}}") && withDetail.includes("{{detail}}"),
      `${file} withDetail must keep both placeholders`,
    );
  }
});
