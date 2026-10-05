import { useMemo, useRef, type RefObject } from "react";
import { useNodes, useReactFlow, useViewport, type Node } from "@xyflow/react";
import { CommentsLayer, CommentPins, type PinProps } from "./comments";
import type { CommentPin } from "./comments-model";

function CanvasPins(props: PinProps) {
  return <CommentPins {...props} camera={useViewport()} />;
}
/** Only mounted with the comment tool; camera updates subscribe at the pins. */
export function CanvasComments(props: { send: (type: string, data?: Record<string, unknown>) => void; requestId: () => string; canvas: RefObject<HTMLDivElement | null> }) {
  const nodes = useNodes<Node<{ screen?: { name: string } }>>();
  const flow = useReactFlow();
  const requested = useRef<string | null>(null);
  const boards = useMemo(() => new Map(nodes.filter(n => n.type === "artboard").map(n => [n.id, n])), [nodes]);
  const focus = (pin: CommentPin) => {
    const board = flow.getNode(pin.screen_id), area = props.canvas.current;
    if (!board || !area) return true;
    if (board.hidden) {
      if (requested.current !== pin.id) { requested.current = pin.id; props.send("select", { screen_id: pin.screen_id }); }
      return false;
    }
    requested.current = null;
    const zoom = Math.max(.4, flow.getZoom());
    void flow.setViewport({ zoom,
      x: area.clientWidth / 2 - (board.position.x + pin.x * (board.width ?? 0)) * zoom,
      y: area.clientHeight / 2 - (board.position.y + pin.y * (board.height ?? 0)) * zoom,
    }, { duration: 0 });
    return true;
  };
  return <CommentsLayer {...props} boards={boards} focus={focus} renderPins={pins => <CanvasPins {...pins} />} />;
}
