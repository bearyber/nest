import type { ButtonHTMLAttributes, ReactNode } from "react";
import Icon, { type IconName } from "./Icon";
import styles from "./ui.module.css";

// Shared native-style controls (build spec §8). No component library.

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "primary";
}

/** A file path that wraps after `\`, `/` and `_` instead of in the middle of a word. */
export function PathText({ path }: { path: string }) {
  return path.split(/(?<=[\\/_])/).map((part, i) => (
    <span key={i}>
      {part}
      <wbr />
    </span>
  ));
}

export function Button({ variant = "default", className, type = "button", ...rest }: ButtonProps) {
  const cls = [styles.button, variant === "primary" ? styles.primary : "", className ?? ""].join(" ");
  return <button type={type} className={cls} {...rest} />;
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
}

/** Small borderless button that shows its outline on hover. `label` is read by screen readers. */
export function IconButton({ icon, label, className, type = "button", ...rest }: IconButtonProps) {
  return (
    <button
      type={type}
      className={`${styles.iconButton} ${className ?? ""}`}
      aria-label={label}
      title={label}
      {...rest}
    >
      <Icon name={icon} />
    </button>
  );
}

interface SegmentedProps {
  label: string;
  options: string[];
  value: string;
  onChange: (value: string) => void;
  /** Fill the available width, with equal-width segments. */
  block?: boolean;
  disabled?: boolean;
}

/** One-of-a-few choice, like Work | Personal. */
export function Segmented({ label, options, value, onChange, block, disabled }: SegmentedProps) {
  return (
    <div
      className={`${styles.segmented} ${block ? styles.block : ""}`}
      role="group"
      aria-label={label}
      aria-disabled={disabled || undefined}
    >
      {options.map((o) => (
        <button
          key={o}
          type="button"
          aria-pressed={o === value}
          disabled={disabled}
          onClick={() => onChange(o)}
        >
          {o}
        </button>
      ))}
    </div>
  );
}

/** Keyboard shortcut hint inside a button, e.g. Ctrl ↵. */
export function Kbd({ children }: { children: ReactNode }) {
  return <span className={styles.kbd}>{children}</span>;
}
