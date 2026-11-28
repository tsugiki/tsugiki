import { resolve } from "node:path";
import { run, stop } from "./run.ts";

const { milestone, description_file: descriptionFile } = process.env;
if (milestone === undefined || !/^[1-9][0-9]*$/.test(milestone)) {
  stop("Pass a positive milestone number to just milestone-description.");
}
if (!descriptionFile) {
  stop("Pass a description file to just milestone-description.");
}

const title = run(
  "Could not update the milestone description.",
  "gh",
  "api",
  "--hostname",
  "github.com",
  "-X",
  "PATCH",
  `repos/tsugiki/tsugiki/milestones/${milestone}`,
  "-F",
  `description=@${resolve(descriptionFile)}`,
  "--jq",
  ".title",
);
console.log(title);
