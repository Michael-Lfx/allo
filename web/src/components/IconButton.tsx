import type { ButtonHTMLAttributes, ReactNode } from "react";

/** Icon-only button: the label is the accessible name, never rendered text. */
export function IconButton({
  label,
  className = "",
  children,
  onClick,
  ...props
}: {
  label: string;
  className?: string;
  children: ReactNode;
  onClick?: () => void;
} & Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label" | "className" | "onClick">) {
  return <button type="button" className={`icon-button ${className}`} aria-label={label} title={label} onClick={onClick} {...props}>{children}</button>;
}
