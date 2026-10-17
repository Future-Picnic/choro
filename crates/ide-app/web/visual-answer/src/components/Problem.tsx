export function Problem({ text }: { text: string }) {
  return (
    <p className="va-problem" role="alert">
      {text}
    </p>
  );
}
