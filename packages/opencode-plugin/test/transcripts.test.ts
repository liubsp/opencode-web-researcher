import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile, rm, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { publishTranscripts } from "../src/transcripts.ts";

test("transcripts publish independent Markdown temp copies and regenerate deleted copies", async () => {
  const root = await mkdtemp(join(tmpdir(), "transcript-links-"));
  try {
    const source = join(root, "original # ü.md");
    await writeFile(source, "Full transcript\n你好");
    const response = () => ({results:[{local_transcript:{markdown:{path:source},archive_markdown:{path:source}}}]});
    const published = await publishTranscripts(response(), join(root, "opencode-temp")) as ReturnType<typeof response>;
    const local = published.results[0].local_transcript;
    assert.notEqual(local.markdown.path, source);
    assert.equal(await readFile(local.markdown.path, "utf8"), "Full transcript\n你好");
    assert.equal(await readFile(fileURLToPath((local.markdown as {path:string,url:string}).url), "utf8"), "Full transcript\n你好");
    assert.equal(local.archive_markdown, undefined);
    await unlink(local.markdown.path);
    await publishTranscripts(response(), join(root, "opencode-temp"));
    assert.equal(await readFile(local.markdown.path, "utf8"), "Full transcript\n你好");
    await writeFile(local.markdown.path, "parent edit");
    assert.equal(await readFile(source, "utf8"), "Full transcript\n你好");
  } finally { await rm(root, {recursive:true,force:true}); }
});
