// Integrated cbm/codebase-memory OpenCode plugin.
// Adds a graph-first reminder and best-effort Grep/Glob context augmentation.

const CBM_BIN = process.env.CBM_BIN || "{{CBM_BIN}}";
const REMINDER = [
  "Code Discovery Protocol (cbm):",
  "1. Use cbm graph tools first for code exploration: search_graph, trace_path, get_code_snippet, query_graph, get_architecture, search_code.",
  "2. Project names use the cbm+ prefix; run index_repository first when a repository is not indexed.",
  "3. Use Grep/Glob/Read for configs and non-code files, and read files before editing them.",
  "4. Use the integrated rlm_* tools for huge logs, long documents, and bounded map-reduce work.",
].join("\n");

let pendingContext = "";

function toolName(input) {
  return String(input?.tool || input?.toolName || input?.tool_name || "").toLowerCase();
}

function toolArgs(input, output) {
  return output?.args || input?.args || input?.tool_input || input?.toolInput || {};
}

function patternFrom(input, output) {
  const args = toolArgs(input, output);
  return args.pattern || args.query || args.path || args.include || "";
}

function cwdFrom(ctx, input) {
  return input?.cwd || input?.directory || ctx?.worktree || ctx?.directory || ".";
}

function appendSystem(output, text) {
  if (!output || typeof output !== "object" || !text) return;
  const block = `\n\n<codebase-memory>\n${text}\n</codebase-memory>`;
  const current = output.system;
  if (typeof current === "string") {
    if (!current.includes(text)) output.system = current + block;
    return;
  }
  if (Array.isArray(current)) {
    const serialized = JSON.stringify(current);
    if (!serialized.includes(text)) current.push({ type: "text", text: block.trim() });
    return;
  }
  if (current && typeof current === "object" && typeof current.content === "string") {
    if (!current.content.includes(text)) current.content += block;
    return;
  }
  output.system = block.trim();
}

async function runCbmAugment(tool, pattern, cwd) {
  if (!CBM_BIN || CBM_BIN.includes("{{CBM_BIN}}")) return "";
  if (typeof Bun === "undefined" || typeof Bun.spawn !== "function") return "";

  const hookTool = tool === "glob" ? "Glob" : "Grep";
  const payload = JSON.stringify({
    tool_name: hookTool,
    tool_input: { pattern },
    cwd,
  });

  const proc = Bun.spawn([CBM_BIN, "hook-augment"], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "ignore",
  });
  proc.stdin.write(payload);
  proc.stdin.end();

  const text = await Promise.race([
    new Response(proc.stdout).text(),
    new Promise((resolve) => setTimeout(() => resolve(""), 600)),
  ]);
  try {
    await proc.exited;
  } catch {
    return "";
  }
  if (!text) return "";

  try {
    const parsed = JSON.parse(text);
    return parsed?.hookSpecificOutput?.additionalContext || "";
  } catch {
    return "";
  }
}

export const CbmCodebaseMemory = async (ctx) => {
  return {
    "experimental.chat.system.transform": async (_input, output) => {
      try {
        appendSystem(output, REMINDER);
        if (pendingContext) {
          appendSystem(output, pendingContext);
          pendingContext = "";
        }
      } catch {
        pendingContext = "";
      }
    },
    "tool.execute.before": async (input, output) => {
      try {
        const name = toolName(input);
        if (name !== "grep" && name !== "glob") return;
        const pattern = String(patternFrom(input, output) || "");
        if (!pattern) return;
        const context = await runCbmAugment(name, pattern, cwdFrom(ctx, input));
        if (!context) return;
        pendingContext = context;
        if (output && typeof output === "object") {
          output.metadata = { ...(output.metadata || {}), cbmContext: context };
        }
        console.error(context);
      } catch {
        pendingContext = "";
      }
    },
  };
};
