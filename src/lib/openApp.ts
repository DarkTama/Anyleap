import type { OpenAppOutcome } from "@/lib/types";

/** Past-tense status text describing where an app was opened. */
export function describeOpenApp(label: string, outcome?: OpenAppOutcome): string {
  switch (outcome) {
    case "flex":
      return `Opened ${label} in your Flex window`;
    case "mirror":
      return `Opened ${label} on the phone (mirror)`;
    case "newWindow":
      return `Opened ${label} in a new window`;
    default:
      return `Opened ${label}`;
  }
}
