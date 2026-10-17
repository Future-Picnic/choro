import type { ReactNode } from "react";

export function StackView({ children }: { children: ReactNode }) {
  return <div className="va-stack">{children}</div>;
}

export function SectionView({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="va-section">
      <h3 className="va-section-title">{title}</h3>
      <div className="va-stack">{children}</div>
    </section>
  );
}

/** Plain text only: no markdown, links, images or HTML from the program. */
export function NoteView({ text, tone }: { text: string; tone?: "neutral" | "caution" | null }) {
  return <p className={`va-note-block${tone === "caution" ? " va-note-caution" : ""}`}>{text}</p>;
}
