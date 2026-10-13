import assert from "node:assert/strict";
import test from "node:test";
import { currentReleaseNames, releaseIdentityName } from "./current-release.ts";

test("current release uses the grab identity after library rename, not quality similarity", () => {
  const names = currentReleaseNames([{
    role: "PRIMARY",
    grabbedReleaseTitle: " Film.2025.1080p.WEB-DL-GROUP ",
    sceneName: "Film.2025.1080p.WEB-DL-OTHER",
    filePath: "/library/Film (2025)/Film.mkv",
  }]);
  assert.ok(names.has(releaseIdentityName("film.2025.1080p.web-dl-group")));
  assert.ok(!names.has(releaseIdentityName("Film.2025.1080p.WEB-DL-OTHER")));
  assert.ok(!names.has("film"));
});

test("only live primary files supplied by the title query identify the current release", () => {
  assert.deepEqual([...currentReleaseNames([
    { role: "additional", grabbedReleaseTitle: "Old.Release", filePath: "/library/old.mkv" },
    { role: "primary", grabbedReleaseTitle: "New.Release", filePath: "/library/new.mkv" },
  ])], ["new.release"]);
  assert.equal(currentReleaseNames([]).size, 0);
});

test("scanned files use scene or original file identity without fuzzy title matching", () => {
  assert.deepEqual([...currentReleaseNames([
    { role: "primary", sceneName: "Show.S01E01-GROUP", filePath: "/library/renamed.mkv" },
    { role: "primary", originalFilePath: "D:\\downloads\\Show.S01E02-GROUP.mkv", filePath: "/library/renamed2.mkv" },
  ])], ["show.s01e01-group", "show.s01e02-group"]);
});
