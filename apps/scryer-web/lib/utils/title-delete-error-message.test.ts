import assert from "node:assert/strict";
import test from "node:test";
import en from "../i18n/locales/en.ts";
import { titleDeleteErrorMessage } from "./title-delete-error-message.ts";
const t = (key: string) => en[key] ?? key;
test("title deletion chooses translations by reason regardless of the sentence", () => {
  const error = { graphQLErrors: [{ message: "unrelated wording", extensions: {
    code: "VALIDATION_ERROR", reason: "TITLE_DELETE_PREVIEW_STALE",
  } }] };
  assert.equal(titleDeleteErrorMessage(error, t, "failed"), en["titleDelete.refusal.preview"]);
});
test("unknown deletion reasons retain the server explanation", () => {
  const error = { graphQLErrors: [{ message: "validation: new condition", extensions: {
    code: "VALIDATION_ERROR", reason: "TITLE_DELETE_FUTURE_CONDITION",
  } }] };
  assert.equal(titleDeleteErrorMessage(error, t, "failed"), "new condition");
});
