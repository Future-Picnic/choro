import type { ComponentPropsWithRef } from "react";
import "./buttons.css";

type Props = ComponentPropsWithRef<"button">;
/** Canonical controls for the trusted Studio canvas, using the host theme. */
export function StageButton({ className = "", primary = false, ...props }: Props & { primary?: boolean }) {
  return <button type="button" {...props} className={`stage-button ${primary ? "primary" : ""} ${className}`} />;
}
export function PinButton({ className = "", ...props }: Props) {
  return <button type="button" {...props} className={`comment-pin nodrag nopan ${className}`} />;
}
export function CommentRowButton({ className = "", ...props }: Props) {
  return <button type="button" {...props} className={`comment-row ${className}`} />;
}
export function StageIconButton({ className = "", ...props }: Props) {
  return <StageButton {...props} className={`stage-icon-button ${className}`} />;
}
