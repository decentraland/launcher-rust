import React from "react";

// Sticker-style icons from the design: flat fill with a dark outline. Inline SVG until the
// Figma exports land; swap the paths, keep the component names.

const OUTLINE = "#2A0B3D";
const ORANGE = "#F6A34F";
const GREEN = "#3EC94D";
const CHECK_RED = "#F25C48";

interface IconProps {
  size?: number;
}

export const WarningIcon: React.FC<IconProps> = ({ size = 56 }) => (
  <svg width={size} height={size} viewBox="0 0 56 56" fill="none">
    <path
      d="M24.5 8.5c1.6-2.7 5.4-2.7 7 0l19 33c1.6 2.7-.3 6-3.5 6H9c-3.2 0-5.1-3.3-3.5-6l19-33Z"
      fill={ORANGE}
      stroke={OUTLINE}
      strokeWidth="3"
      strokeLinejoin="round"
    />
    <path
      d="M28 19v13"
      stroke={OUTLINE}
      strokeWidth="4"
      strokeLinecap="round"
    />
    <circle cx="28" cy="39" r="2.5" fill={OUTLINE} />
  </svg>
);

export const BugIcon: React.FC<IconProps> = ({ size = 56 }) => (
  <svg width={size} height={size} viewBox="0 0 56 56" fill="none">
    {/* legs */}
    <path
      d="M14 22 8 17M14 30H7M14 38l-6 5M42 22l6-5M42 30h7M42 38l6 5"
      stroke={OUTLINE}
      strokeWidth="3"
      strokeLinecap="round"
    />
    {/* head */}
    <path
      d="M20 14a8 8 0 0 1 16 0v3H20v-3Z"
      fill={ORANGE}
      stroke={OUTLINE}
      strokeWidth="3"
      strokeLinejoin="round"
    />
    <path
      d="M22 8l-3-4M34 8l3-4"
      stroke={OUTLINE}
      strokeWidth="3"
      strokeLinecap="round"
    />
    {/* body */}
    <rect
      x="14"
      y="17"
      width="28"
      height="30"
      rx="14"
      fill={ORANGE}
      stroke={OUTLINE}
      strokeWidth="3"
    />
    <path
      d="M28 17v30M14 30h28"
      stroke={OUTLINE}
      strokeWidth="2.5"
      strokeLinecap="round"
    />
  </svg>
);

export const SuccessIcon: React.FC<IconProps> = ({ size = 56 }) => (
  <svg width={size} height={size} viewBox="0 0 56 56" fill="none">
    <circle
      cx="28"
      cy="28"
      r="22"
      fill={GREEN}
      stroke={OUTLINE}
      strokeWidth="3"
    />
    <path
      d="M17 28.5 24.5 36 39 21"
      stroke="#FFFFFF"
      strokeWidth="5"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

export const CloseIcon: React.FC<IconProps> = ({ size = 12 }) => (
  <svg width={size} height={size} viewBox="0 0 12 12" fill="none">
    <path
      d="M2 2l8 8M10 2l-8 8"
      stroke="#FFFFFF"
      strokeWidth="2.2"
      strokeLinecap="round"
    />
  </svg>
);

// White rounded box; the check is red when on. Passed as MUI Checkbox `icon` / `checkedIcon`.
export const CheckboxBox: React.FC<{ checked: boolean }> = ({ checked }) => (
  <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
    <rect x="1" y="1" width="22" height="22" rx="5" fill="#FFFFFF" />
    {checked ? (
      <path
        d="M6 12.5 10 16.5 18 8"
        stroke={CHECK_RED}
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    ) : null}
  </svg>
);
