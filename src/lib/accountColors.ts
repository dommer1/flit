// Preset accent colors an account can be tagged with. Kept in one place so
// the settings picker and the message-list / sidebar indicators agree on the
// palette. Stored as plain hex strings on the account (color column).

export interface AccountColor {
  name: string;
  value: string;
}

export const ACCOUNT_COLORS: readonly AccountColor[] = [
  { name: "Red", value: "#ff453a" },
  { name: "Orange", value: "#ff9f0a" },
  { name: "Yellow", value: "#ffd60a" },
  { name: "Green", value: "#30d158" },
  { name: "Teal", value: "#40c8e0" },
  { name: "Blue", value: "#0a84ff" },
  { name: "Purple", value: "#bf5af2" },
  { name: "Pink", value: "#ff375f" },
];
