(* The placement rules (F06, F07, F08) as computable checks, and what the push fallback (the
   answer None) still does against them.
   1. Tiles reassigned: no layout that puts the moved windows where the move put them, keeps every
      other span and overlaps nothing reassigns fewer.
   2. A rearrangement splits no island: two windows connected without the moved ones before it
      stay connected after it, through any window; windows connect through a shared side.
   3. A window that makes room keeps its row or its column; the moved windows are exempt.
   src/place.rs enforces rule 2 only: it is Canvas.splits, which place (accepts), close and shrink
   check. Rules 1 and 3 are measures, not constraints: the search minimises tiles reassigned plus
   distance_weight per tile of area moved one tile plus diagonal_weight per tile of area of each
   window that changed both its row and its column, so it may reassign more tiles than the fewest
   to move windows less, and it charges a diagonal move instead of refusing it. tests/rules.rs's
   exact search reports how often each measure is met (ignored, run by hand). *)
From Stdlib Require Import ZArith List Bool.
Import ListNotations.
From NewmCanvas Require Import Canvas.
Open Scope Z_scope.

Definition inside x y r := (ri r <=? x) && (x <? right r) && (rj r <=? y) && (y <? bottom r).
Definition owner ws x y := option_map wid (find (fun w => inside x y (wrect w)) ws).
Definition same_owner (a b : option nat) := match a, b with
  | Some a, Some b => Nat.eqb a b | None, None => true | _, _ => false end.
Definition bbox ws := fold_right (fun w '(x0, y0, x1, y1) =>
  (Z.min x0 (ri (wrect w)), Z.min y0 (rj (wrect w)), Z.max x1 (right (wrect w)), Z.max y1 (bottom (wrect w))))
  (0, 0, 0, 0) ws.
(* The tiles whose owner, a window or none, differs between two layouts. *)
Definition displaced ws ws' :=
  let '(x0, y0, x1, y1) := bbox (ws ++ ws') in
  length (filter (fun p => negb (same_owner (owner ws (fst p) (snd p)) (owner ws' (fst p) (snd p))))
    (flat_map (fun x => map (fun y => (x, y)) (range y0 y1)) (range x0 x1))).

(* Rule 2 broken for a and b: connected without the movers before, not connected after, through
   any window, the movers in their new places included; Canvas.splits asks it of every pair. *)
Definition split_pair ws ws' movers a b := connected (but movers ws) a b && negb (connected ws' a b).

(* A layout the rule 1 search may pick: nothing overlaps, every window keeps its span. *)
Definition no_overlap ws := forallb (fun a => forallb (fun b =>
  Nat.eqb (wid a) (wid b) || negb (overlaps (wrect a) (wrect b))) ws) ws.
Definition spans_as ws ws' := (length ws =? length ws')%nat && forallb (fun w' =>
  match window_of (wid w') ws with Some w => (rw (wrect w) =? rw (wrect w')) && (rh (wrect w) =? rh (wrect w')) | None => false end) ws'.

Definition on_wide os := run shipped (canvas_new 2560 1440) os.
Definition after_op os o := match on_wide os with Some c => option_map (fun c' => (windows c, windows c')) (step shipped c o) | None => None end.

(* F06, repaired by place.rs; the push fallback still breaks rule 1: moving 1 right into 2 pushes 2
   along and reassigns 3 tiles; 2 taking the tile 1 left reassigns 2, overlaps nothing, keeps both
   spans and splits nothing, the layout move_by takes from the engine (binds_canvas_v2). Dropping the
   2x1 window 1 at (-2, 1) closes the lake it left (5 tiles); leaving 2 where it was takes 4. *)
Example move_reassigns_more_than_it_must :
  match after_op [Add 1 (100, 100) None; Add 2 (100, 100) None] (MoveBy 1 Right None) with
  | Some (ws, ws') => let alt := [Window 1 (Rect 1 0 1 1) (100, 100); Window 2 (Rect 0 0 1 1) (100, 100)] in
      displaced ws ws' = 3%nat /\ displaced ws alt = 2%nat /\ no_overlap alt = true /\ spans_as ws alt = true /\
      rect_of 1 alt = rect_of 1 ws' /\ connected alt 1 2 = true
  | None => False end /\
  match after_op [Add 1 (800, 300) None; Add 2 (100, 100) None] (MoveTo 1 (-2) 1 None None) with
  | Some (ws, ws') => let alt := [Window 1 (Rect (-2) 1 2 1) (800, 300); Window 2 (Rect 2 0 1 1) (100, 100)] in
      displaced ws ws' = 5%nat /\ displaced ws alt = 4%nat /\ no_overlap alt = true /\ spans_as ws alt = true /\
      rect_of 1 alt = rect_of 1 ws'
  | None => False end.
Proof. vm_compute. repeat split; reflexivity. Qed.

(* F07, repaired: closing 1 slid 2 into its lake, away from 3, which touched only 2. That fill
   splits the island, so remove leaves the gap open and 2 keeps its tiles (C51). *)
Example rearrangement_splits_an_island :
  match after_op [AddAt 1 (800, 300) (Rect 0 0 2 1); AddAt 2 (800, 300) (Rect 2 0 2 1); AddAt 3 (100, 100) (Rect 2 (-1) 1 1)]
    (Remove 1 None) with
  | Some (ws, ws') => split_pair ws (closed (Rect 0 0 2 1) (without 1 ws)) [1%nat] 2 3 = true /\
      splits ws (closed (Rect 0 0 2 1) (without 1 ws)) [1%nat] = true /\ ws' = without 1 ws
  | None => False end.
Proof. vm_compute. repeat split. Qed.

(* Rule 3 broken: window i, not a mover, changed both its column and its row. *)
Definition bent ws ws' movers i := negb (mem i movers) && match rect_of i ws, rect_of i ws' with
  | Some a, Some b => negb (ri a =? ri b) && negb (rj a =? rj b) | _, _ => false end.

(* F08, repaired: move_to puts 2 back on its own tile (2, 0). The push fallback (moved_to) takes 2
   out, which slides 1 up into the gap from (1, 1), and putting 2 back pushes 1 right, to (3, 0): a
   diagonal move for nothing. move_to now changes nothing when the target is its own rectangle. *)
Example drop_moves_a_window_diagonally :
  match after_op [Add 1 (800, 300) None; Add 2 (100, 100) None; MoveBy 1 Down None; MoveBy 1 Right None] (MoveTo 2 2 0 None None) with
  | Some (ws, ws') => rect_of 2 ws = Some (Rect 2 0 1 1) /\ rect_of 1 ws = Some (Rect 1 1 2 1) /\
      option_map (rect_of 1) (moved_to 2 2 0 ws) = Some (Some (Rect 3 0 2 1)) /\
      option_map (fun ws2 => bent ws ws2 [2%nat] 1) (moved_to 2 2 0 ws) = Some true /\ ws' = ws
  | None => False end.
Proof. vm_compute. repeat split. Qed.
