import { describe, expect, it } from "vitest";
import { describeOpenApp } from "./openApp";

describe("describeOpenApp", () => {
  it("describes each outcome", () => {
    expect(describeOpenApp("Chat", "flex")).toBe("Opened Chat in your Flex window");
    expect(describeOpenApp("Chat", "mirror")).toBe("Opened Chat on the phone (mirror)");
    expect(describeOpenApp("Chat", "newWindow")).toBe("Opened Chat in a new window");
    expect(describeOpenApp("Chat")).toBe("Opened Chat");
  });
});
