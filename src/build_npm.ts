import { build, emptyDir } from "https://deno.land/x/dnt@0.40.0/mod.ts";
import fs from "node:fs";

const denoJson = JSON.parse(Deno.readTextFileSync("./deno.json"));

await emptyDir("./build");

await build({
  entryPoints: [
    {
      kind: "bin",
      name: "openape",
      path: "./src/index.ts",
    },
  ],
  outDir: "./build",
  shims: {
    deno: true,
  },
  compilerOptions: {
    lib: ["ES2022", "DOM"],
  },
  test: false,
  package: {
    name: "openape",
    version: denoJson.version,
    description: denoJson.description,
    license: "MIT",
    repository: {
      type: "git",
      url: "git+https://github.com/mo7yw4ng/openape.git",
    },
    bugs: {
      url: "https://github.com/mo7yw4ng/openape/issues",
    },
    engines: {
      node: ">=18.0.0",
    },
    keywords: ["ilearning", "cycu", "cli", "headless", "automation", "playwright"],
  },
  postBuild() {
    // Copy LICENSE and README
    Deno.copyFileSync("LICENSE", "build/LICENSE");
    Deno.copyFileSync("README.md", "build/README.md");

    // Bundle the skills directory so `openape skills install` works after npm install
    const skillsSrc = "./skills";
    const skillsDest = "./build/skills";
    if (fs.existsSync(skillsSrc)) {
      copyDirSync(skillsSrc, skillsDest);
    }
  },
});

function copyDirSync(src: string, dest: string) {
  if (!fs.existsSync(dest)) {
    fs.mkdirSync(dest, { recursive: true });
  }
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const srcPath = `${src}/${entry.name}`;
    const destPath = `${dest}/${entry.name}`;
    if (entry.isDirectory()) {
      copyDirSync(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}
