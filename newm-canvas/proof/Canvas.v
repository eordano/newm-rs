(* The tiled-window decisions of newm-canvas/src/lib.rs, one definition per Rust function.
   Geometry is over Z in tiles; logical pixels are Z in a fixed fraction of a pixel. *)
From Stdlib Require Import ZArith List Bool Lia.
Import ListNotations.
Open Scope Z_scope.

Record rect := Rect { ri : Z; rj : Z; rw : Z; rh : Z }.
Definition right r := ri r + rw r.
Definition bottom r := rj r + rh r.
Definition rows_overlap a b := (rj a <? bottom b) && (rj b <? bottom a).
Definition cols_overlap a b := (ri a <? right b) && (ri b <? right a).
Definition overlaps a b := rows_overlap a b && cols_overlap a b.
Definition shifted r di dj := Rect (ri r + di) (rj r + dj) (rw r) (rh r).

Inductive dir := Left | Right | Up | Down.
Definition delta d := match d with Left => (-1, 0) | Right => (1, 0) | Up => (0, -1) | Down => (0, 1) end.
Definition opposite d := match d with Left => Right | Right => Left | Up => Down | Down => Up end.
Inductive edge := ELeft | ERight | ETop | EBottom.
Definition outward e := match e with ELeft => Left | ERight => Right | ETop => Up | EBottom => Down end.

(* wmin is the app's minimum logical size (min_w, min_h). *)
Record window := Window { wid : nat; wrect : rect; wmin : Z * Z }.
Definition set_rect r w := Window (wid w) r (wmin w).

(* Config and the float decisions: the on-screen tile at each zoom (the camera; zoom 0 is 1:1, so
   tile_w e 0 is base_tile), the tiles a minimum needs (from base_tile: no zoom), EPS. *)
Record env := Env {
  tile_w : Z -> Z; tile_h : Z -> Z; need : Z * Z -> Z * Z;
  eps : Z; zoom_min : Z; zoom_max : Z }.

(* What the code establishes about them: need is at least one tile (.max(1.0)), clamp's bounds
   are ordered (i32::clamp panics otherwise), EPS is not negative. *)
Definition good e := 0 <= eps e /\ zoom_min e <= zoom_max e /\
  forall m, 1 <= fst (need e m) /\ 1 <= snd (need e m).

(* recent: window ids, the most recently focused first. *)
Record canvas := Canvas { windows : list window; focus : option nat; zoom : Z; view : Z * Z; screen : Z * Z;
  recent : list nat }.
Definition with_windows c ws := Canvas ws (focus c) (zoom c) (view c) (screen c) (recent c).
Definition with_view c v := Canvas (windows c) (focus c) (zoom c) v (screen c) (recent c).
(* set_focus: the focus, moved to the front of recent (retain, then insert at 0). *)
Definition forget i l := filter (fun r => negb (Nat.eqb r i)) l.
Definition set_focus c i := Canvas (windows c) (Some i) (zoom c) (view c) (screen c) (i :: forget i (recent c)).

Definition window_of i ws := find (fun w => Nat.eqb (wid w) i) ws.
Definition rect_of i ws := option_map wrect (window_of i ws).
Definition ids ws := map wid ws.
Definition mem i l := existsb (Nat.eqb i) l.
Definition free r except ws := forallb (fun w => mem (wid w) except || negb (overlaps (wrect w) r)) ws.
Definition update i (f : window -> window) ws := map (fun w => if Nat.eqb (wid w) i then f w else w) ws.

(* fits e z: a span fits on the screen at zoom z; fits_on_screen is fits e (zoom c), fits_at_1to1 fits e 0. *)
Definition fits e z (s : Z * Z) w h := (w * tile_w e z <=? fst s + eps e) && (h * tile_h e z <=? snd s + eps e).

Definition least e view start len screen :=
  if (screen + eps e <? len) || (start <? view) then start
  else if view + screen <? start + len then start + len - screen else view.
Definition ensure_visible e c r :=
  with_view c (least e (fst (view c)) (ri r * tile_w e (zoom c)) (rw r * tile_w e (zoom c)) (fst (screen c)),
               least e (snd (view c)) (rj r * tile_h e (zoom c)) (rh r * tile_h e (zoom c)) (snd (screen c))).
Definition focus_rect c := match focus c with Some f => rect_of f (windows c) | None => None end.
Definition ensure_focus_visible e c := match focus_rect c with Some r => ensure_visible e c r | None => c end.
Definition is_visible e c r :=
  let x := ri r * tile_w e (zoom c) - fst (view c) in let y := rj r * tile_h e (zoom c) - snd (view c) in
  (- eps e <=? x) && (- eps e <=? y) && (x + rw r * tile_w e (zoom c) <=? fst (screen c) + eps e)
  && (y + rh r * tile_h e (zoom c) <=? snd (screen c) + eps e).

(* physical: both sides come from the grid line they sit on; edge is edge_x or edge_y at one
   scale and view, any function of the line. *)
Definition physical (ex ey : Z -> Z) r :=
  (ex (ri r), ey (rj r), ex (right r) - ex (ri r), ey (bottom r) - ey (rj r)).

(* push: the other windows sorted by (lead, id), lead being the edge that faces d; each moves
   past the mover if it overlaps it, then past every swept window that shares its cross range and
   that it is behind, round after round until it stops moving (settle_stops bounds the rounds). *)
Definition past d p r := match d with
  | Left => Rect (ri p - rw r) (rj r) (rw r) (rh r)
  | Right => Rect (right p) (rj r) (rw r) (rh r)
  | Up => Rect (ri r) (rj p - rh r) (rw r) (rh r)
  | Down => Rect (ri r) (bottom p) (rw r) (rh r) end.
Definition lead d r := match d with Right => ri r | Left => - right r | Down => rj r | Up => - bottom r end.
Definition shares d p r := match d with Left | Right => rows_overlap p r | Up | Down => cols_overlap p r end.
Definition behind d r p := match d with
  | Right => ri r <? right p | Left => ri p <? right r | Down => rj r <? bottom p | Up => rj p <? bottom r end.
Definition round d m swept r := fold_left (fun r p => if shares d p r && behind d r p then past d p r else r)
  swept (if overlaps r m then past d m r else r).
Fixpoint settle d m swept (n : nat) r := match n with O => r | S n' => settle d m swept n' (round d m swept r) end.
Fixpoint sweep d m swept order := match order with
  | [] => []
  | w :: o => let r := settle d m swept (S (length swept)) (wrect w) in set_rect r w :: sweep d m (swept ++ [r]) o end.
Definition sooner d a b := (lead d (wrect a) <? lead d (wrect b)) ||
  (lead d (wrect a) =? lead d (wrect b)) && (wid a <? wid b)%nat.
Fixpoint insert d w l := match l with [] => [w] | x :: l' => if sooner d x w then x :: insert d w l' else w :: l end.
Definition order d m ws := fold_right (insert d) [] (filter (fun w => negb (Nat.eqb (wid w) m)) ws).
Definition put done w := match rect_of (wid w) done with Some r => set_rect r w | None => w end.
Definition push d m ws := match rect_of m ws with
  | None => Some ws
  | Some mr => Some (map (put (sweep d mr [] (order d m ws))) ws) end.

Definition touches side x p := match side with
  | Right => (ri x =? right p) && rows_overlap x p
  | Left => (right x =? ri p) && rows_overlap x p
  | Down => (rj x =? bottom p) && cols_overlap x p
  | Up => (bottom x =? rj p) && cols_overlap x p end.
Fixpoint chain_loop side ws (pops : nat) (q : list rect) (ch : list nat) :=
  match q, pops with
  | p :: q', S pops' =>
      let '(ch', q'') := fold_left (fun '(ch, q) w =>
          if negb (mem (wid w) ch) && touches side (wrect w) p then (ch ++ [wid w], q ++ [wrect w])
          else (ch, q)) ws (ch, q') in
      chain_loop side ws pops' q'' ch'
  | _, _ => ch end.
Definition chain freed side ws := chain_loop side ws (S (length ws)) [freed] [].

Definition slid ch di dj k ws :=
  map (fun w => if mem (wid w) ch then set_rect (shifted (wrect w) (di * k) (dj * k)) w else w) ws.
Definition slides ch di dj k ws :=
  forallb (fun w => negb (mem (wid w) ch) || free (shifted (wrect w) (di * k) (dj * k)) ch ws) ws.
Fixpoint slide ch di dj (n : nat) ws := match n with
  | O => ws
  | S n' => if slides ch di dj (Z.of_nat n) ws then slid ch di dj (Z.of_nat n) ws else slide ch di dj n' ws end.
Definition fill freed side ws :=
  match chain freed side ws with
  | [] => ws
  | ch => let '(di, dj) := delta (opposite side) in
          slide ch di dj (Z.to_nat (if di =? 0 then rh freed else rw freed)) ws end.

(* place.rs: windows touch when they share a side of positive length; grow is the closure of
   touches from seen (islands, with fuel). splits: two windows but the movers, connected without
   them before, are not connected after; is_bridge: i's neighbours fall on different islands
   without it. *)
Definition touch a b := ((right a =? ri b) || (right b =? ri a)) && rows_overlap a b ||
  ((bottom a =? rj b) || (bottom b =? rj a)) && cols_overlap a b.
Fixpoint grow (n : nat) ws (seen : list nat) := match n with O => seen | S n' =>
  grow n' ws (seen ++ map wid (filter (fun w => negb (mem (wid w) seen) &&
    existsb (fun v => mem (wid v) seen && touch (wrect v) (wrect w)) ws) ws)) end.
Definition connected ws a b := mem b (grow (length ws) ws [a]).
Definition but movers ws := filter (fun w => negb (mem (wid w) movers)) ws.
Definition splits ws ws' movers := let bs := ids (but movers ws) in
  existsb (fun a => existsb (fun b => (a <? b)%nat && connected (but movers ws) a b && mem a (ids ws') && mem b (ids ws')
    && negb (connected ws' a b)) bs) bs.
Definition is_bridge ws i := match window_of i ws with None => false | Some w =>
  let near := ids (filter (fun o => negb (Nat.eqb (wid o) i) && touch (wrect o) (wrect w)) ws) in
  existsb (fun a => existsb (fun b => negb (connected (but [i] ws) a b)) near) near end.
(* The engine's answer is a free choice that place checks (accepts): one rectangle per window,
   each target on its rectangle, every other span kept, no two overlapping, no island split.
   gathered: gather's moves, kept only when every span is kept and nothing overlaps. *)
Fixpoint pairwise (rs : list rect) := match rs with
  | [] => true | r :: rs' => forallb (fun q => negb (overlaps r q)) rs' && pairwise rs' end.
Definition relayout ws (l : list rect) := map (fun p => set_rect (snd p) (fst p)) (combine ws l).
Definition span_eq a b := (rw a =? rw b) && (rh a =? rh b).
Definition rect_eqb a b := (ri a =? ri b) && (rj a =? rj b) && span_eq a b.
Definition target_of i (ts : list (nat * rect)) := option_map snd (find (fun t => Nat.eqb (fst t) i) ts).
Definition accepts ws ts l := (length l =? length ws)%nat && forallb (fun p => match target_of (wid (fst p)) ts with
  | Some t => rect_eqb (snd p) t | None => span_eq (snd p) (wrect (fst p)) end) (combine ws l) &&
  pairwise l && negb (splits ws (relayout ws l) (map fst ts)).
Definition placed_by ws ts (p : option (list rect)) := match p with
  | Some l => if accepts ws ts l then Some (relayout ws l) else None | None => None end.
Definition gathered ws (g : option (list rect)) := match g with
  | Some l => if (length l =? length ws)%nat && forallb (fun p => span_eq (snd p) (wrect (fst p))) (combine ws l) && pairwise l
      then relayout ws l else ws
  | None => ws end.

(* enforce_min's two steps: widen to need, then heighten. Each is grow: the engine's answer for
   that window and step (a m Right, a m Down) when place accepts it, else the push along the step. *)
Definition answers := nat -> dir -> option (list rect).
Definition grow_to ws m t d (p : option (list rect)) :=
  match placed_by ws [(m, t)] p with Some ws' => Some ws' | None => push d m (update m (set_rect t) ws) end.
Definition widen e (a : answers) m ws := match window_of m ws with None => Some ws | Some w =>
  let r := wrect w in
  if rw r <? fst (need e (wmin w)) then grow_to ws m (Rect (ri r) (rj r) (fst (need e (wmin w))) (rh r)) Right (a m Right)
  else Some ws end.
Definition heighten e (a : answers) m ws := match window_of m ws with None => Some ws | Some w =>
  let r := wrect w in
  if rh r <? snd (need e (wmin w)) then grow_to ws m (Rect (ri r) (rj r) (rw r) (snd (need e (wmin w)))) Down (a m Down)
  else Some ws end.
Definition enforce_min e a m ws := match widen e a m ws with None => None | Some ws1 => heighten e a m ws1 end.
(* The answers that place never accepts: every step pushes, as the fixed scenarios assume. *)
Definition pushes : answers := fun _ _ => None.

(* open_span: the wanted span cut to the most tiles the screen holds at 1:1, never below need. *)
Definition open_span e (s : Z * Z) mn (want : option (Z * Z)) :=
  let '(nw, nh) := need e mn in match want with
  | None => (nw, nh)
  | Some (ww, wh) => (Z.max (Z.min ww ((fst s + eps e) / tile_w e 0)) nw, Z.max (Z.min wh ((snd s + eps e) / tile_h e 0)) nh) end.
(* spot: the free spot nearest f by gap (a corner-only contact is 1 away), then the least view
   scroll at the current zoom (|least - view| per axis), then side (right, below, left, above),
   then |dj| + |di|; i ascending, then j, a later spot taken only when strictly better. Rust's
   loop over d is unbounded: the model stops after far + 2 rounds, far being the farthest right
   edge past f's; the spot at right f + far in f's row is free with gap at most far + 1. *)
Definition gap f r :=
  let dx := Z.max (Z.max (ri f - right r) (ri r - right f)) 0 in
  let dy := Z.max (Z.max (rj f - bottom r) (rj r - bottom f)) 0 in
  dx + dy + (if (dx =? 0) && (dy =? 0) && negb (touch r f) then 1 else 0).
Definition scroll e c r :=
  Z.abs (least e (fst (view c)) (ri r * tile_w e (zoom c)) (rw r * tile_w e (zoom c)) (fst (screen c)) - fst (view c)) +
  Z.abs (least e (snd (view c)) (rj r * tile_h e (zoom c)) (rh r * tile_h e (zoom c)) (snd (screen c)) - snd (view c)).
Definition side f r := if right f <=? ri r then 0 else if bottom f <=? rj r then 1 else if right r <=? ri f then 2 else 3.
Definition key e c f r := (scroll e c r, side f r, Z.abs (rj r - rj f) + Z.abs (ri r - ri f)).
Definition better e (k b : Z * Z * Z) := let '(k0, k1, k2) := k in let '(b0, b1, b2) := b in
  (k0 + eps e <? b0) || (k0 <=? b0 + eps e) && ((k1 <? b1) || (k1 =? b1) && (k2 <? b2)).
Definition range a b := map (fun k => a + Z.of_nat k) (seq 0 (Z.to_nat (b - a))).
Definition cands f w h d := flat_map (fun i => map (fun j => Rect i j w h) (range (rj f - h - d) (bottom f + d + 1)))
  (range (ri f - w - d) (right f + d + 1)).
Fixpoint pick e c f d best l := match l with [] => best | r :: l' =>
  pick e c f d (if (gap f r =? d) && free r [] (windows c) then
    match best with Some b => if better e (key e c f r) (key e c f b) then Some r else best | None => Some r end
    else best) l' end.
Fixpoint search e c f w h d (n : nat) := match n with O => None | S n' =>
  match pick e c f d None (cands f w h d) with Some r => Some r | None => search e c f w h (d + 1) n' end end.
Definition spot e c f w h :=
  let far := fold_right (fun o m => Z.max (right (wrect o)) m) (right f) (windows c) - right f in
  match search e c f w h 0 (S (S (Z.to_nat far))) with Some r => r | None => Rect (right f + far) (rj f) w h end.
(* add never pushes: the new window takes a free spot beside the focus. *)
Definition add e c i mn want : option canvas :=
  if mem i (ids (windows c)) then None else
  let '(w, h) := open_span e (screen c) mn want in
  let r := match focus_rect c with None => Rect 0 0 w h | Some f => spot e c f w h end in
  Some (ensure_focus_visible e (set_focus (with_windows c (windows c ++ [Window i r mn])) i)).
(* spot_near: rings of tiles around a's, d = 0 to far, each sorted by (di^2 + dj^2, dj, di): the
   nearest, then the upper, then the left. far is the farthest right edge past a's column, at least
   0 (unwrap_or(0).max(0)), so the spot d = far along a's row is free and ends the search. *)
Definition nearer (p q : Z * Z) :=
  match Z.compare (fst p * fst p + snd p * snd p) (fst q * fst q + snd q * snd q), Z.compare (snd p) (snd q),
    Z.compare (fst p) (fst q) with Lt, _, _ | Eq, Lt, _ | Eq, Eq, Lt => true | _, _, _ => false end.
Fixpoint ins p l := match l with [] => [p] | q :: l' => if nearer q p then q :: ins p l' else p :: l end.
Definition ring d := let z := Z.of_nat d in fold_right ins [] (match d with O => [(0, 0)] | S _ =>
  flat_map (fun di => [(di, - z); (di, z)]) (range (- z) (z + 1)) ++ flat_map (fun dj => [(- z, dj); (z, dj)]) (range (1 - z) z) end).
Definition far_past ws a := fold_right (fun o m => Z.max (right (wrect o)) m) (ri a) ws - ri a.
Definition near ws a w h :=
  map (fun p => Rect (ri a + fst p) (rj a + snd p) w h) (flat_map ring (seq 0 (S (Z.to_nat (far_past ws a))))).
Definition spot_near ws a w h := match find (fun r => free r [] ws) (near ws a w h) with
  | Some r => r | None => Rect (ri a + far_past ws a) (rj a) w h end.
(* add_at puts a window back at a saved rectangle a, at open_span of a's span; never pushes. *)
Definition add_at e c i mn a : option canvas :=
  if mem i (ids (windows c)) then None else
  let '(w, h) := open_span e (screen c) mn (Some (rw a, rh a)) in
  Some (ensure_focus_visible e (set_focus (with_windows c (windows c ++ [Window i (spot_near (windows c) a w h) mn])) i)).

Definition center2 r := (2 * ri r + rw r, 2 * rj r + rh r).
Definition dist2 (a b : Z * Z) := (fst a - fst b) ^ 2 + (snd a - snd b) ^ 2.
Definition nearest r ws :=
  let better w best := match best with None => true | Some b =>
    match Z.compare (dist2 (center2 r) (center2 (wrect w))) (dist2 (center2 r) (center2 (wrect b))) with
    | Lt => true | Eq => (wid w <? wid b)%nat | Gt => false end end in
  option_map wid (fold_left (fun best w => if better w best then Some w else best) ws None).
Definition without i ws := filter (fun w => negb (Nat.eqb (wid w) i)) ws.
Definition closed freed ws := fill freed Down (fill freed Right ws).

(* remove: the lake filled unless that splits an island; when i was a bridge, gather's answer g;
   closing the focus focuses the head of recent, else (no window left) nearest. *)
Definition closing ws i freed g :=
  let filled := closed freed (without i ws) in
  let ws1 := if splits ws filled [i] then without i ws else filled in
  if is_bridge ws i then gathered ws1 g else ws1.
Definition remove e c i g : option canvas :=
  match rect_of i (windows c) with None => Some c | Some freed =>
  let ws := closing (windows c) i freed g in
  let c1 := Canvas ws (focus c) (zoom c) (view c) (screen c) (forget i (recent c)) in
  let c2 := match focus c with
    | Some f => if Nat.eqb f i then
        match match recent c1 with r :: _ => Some r | [] => nearest freed ws end with
        | Some n => set_focus c1 n | None => Canvas ws None (zoom c) (view c) (screen c) (recent c1) end
      else c1
    | None => c1 end in
  Some (ensure_focus_visible e c2) end.

Definition grown ed r d := match ed with
  | ELeft => Rect (ri r - d) (rj r) (rw r + d) (rh r)
  | ERight => Rect (ri r) (rj r) (rw r + d) (rh r)
  | ETop => Rect (ri r) (rj r - d) (rw r) (rh r + d)
  | EBottom => Rect (ri r) (rj r) (rw r) (rh r + d) end.
(* The strip a shrink by k frees, and the side the lake would be closed from. *)
Definition strip ed r new k := match ed with
  | ELeft => (Rect (ri r) (rj r) k (rh r), Left)
  | ERight => (Rect (right new) (rj r) k (rh r), Right)
  | ETop => (Rect (ri r) (rj r) (rw r) k, Up)
  | EBottom => (Rect (ri r) (bottom new) (rw r) k, Down) end.
Definition refused e c w new d :=
  let '(nw, nh) := need e (wmin w) in
  (d =? 0) || (rw new <? nw) || (rh new <? nh) || ((0 <? d) && negb (fits e 0 (screen c) (rw new) (rh new))).
(* A grow takes the engine's answer p when place accepts it, else pushes outward; a shrink fills
   its strip unless that splits an island. *)
Definition resize e c i ed d p : option (bool * canvas) :=
  match window_of i (windows c) with None => Some (false, c) | Some w =>
  let new := grown ed (wrect w) d in
  if refused e c w new d then Some (false, c) else
  let ws1 := update i (set_rect new) (windows c) in
  match (if 0 <? d then match placed_by (windows c) [(i, new)] p with Some ws => Some ws | None => push (outward ed) i ws1 end
         else let '(freed, side) := strip ed (wrect w) new (- d) in
           Some (if splits (windows c) (fill freed side ws1) [i] then ws1 else fill freed side ws1)) with
  | None => None
  | Some ws => Some (true, ensure_focus_visible e (with_windows c ws)) end end.

(* swapped: where a swap along d puts the focus a and its neighbour b, both at their spans. Along
   d the pair keeps its extent (the focus ends where b ended, b starts where the focus started);
   across d they trade their first row or column. With equal spans it exchanges the rectangles. *)
Definition swapped d a b := match d with
  | Right => (Rect (right b - rw a) (rj b) (rw a) (rh a), Rect (ri a) (rj a) (rw b) (rh b))
  | Left => (Rect (ri b) (rj b) (rw a) (rh a), Rect (right a - rw b) (rj a) (rw b) (rh b))
  | Down => (Rect (ri b) (bottom b - rh a) (rw a) (rh a), Rect (ri a) (rj a) (rw b) (rh b))
  | Up => (Rect (ri b) (rj b) (rw a) (rh a), Rect (ri a) (bottom a - rh b) (rw b) (rh b)) end.
(* swap with neighbour(focus, d), which the model takes as n. The fallback (swap_push): n steps out
   while the focus lands and pushes along d, then n comes back last and pushes the other way. *)
Definition swap_push e c n d : option canvas :=
  match focus c with None => Some c | Some f =>
  if Nat.eqb f n then Some c else
  match rect_of f (windows c), window_of n (windows c) with
  | Some ra, Some wn =>
      let '(fa, nb) := swapped d ra (wrect wn) in
      match push d f (update f (set_rect fa) (without n (windows c))) with
      | None => None
      | Some ws1 => match push (opposite d) n (ws1 ++ [set_rect nb wn]) with
          | None => None
          | Some ws2 => Some (ensure_focus_visible e (with_windows c ws2)) end end
  | _, _ => Some c end end.

Definition swap e c n d p : option canvas :=
  match focus c with None => Some c | Some f =>
  if Nat.eqb f n then Some c else
  match rect_of f (windows c), window_of n (windows c) with
  | Some ra, Some wn => let '(fa, nb) := swapped d ra (wrect wn) in
      match placed_by (windows c) [(f, fa); (n, nb)] p with
      | Some ws => Some (ensure_focus_visible e (with_windows c ws))
      | None => swap_push e c n d end
  | _, _ => Some c end end.

Definition take i ws := match window_of i ws with None => None | Some w =>
  Some (w, closed (wrect w) (without i ws)) end.
Definition edge_i d w o rest := match d with
  | Left => fold_right (fun x acc => Z.min (ri (wrect x)) acc) (ri (wrect o)) rest - rw (wrect w)
  | _ => fold_right (fun x acc => Z.max (right (wrect x)) acc) (right (wrect o)) rest end.
(* send_to_edge's fallback (send_push) takes the focus out and measures the edge after the take. *)
Definition send_push e c d : option canvas :=
  match focus c with None => Some c | Some f =>
  if (length (windows c) <? 2)%nat || negb (match d with Left | Right => true | _ => false end) then Some c else
  match take f (windows c) with
  | None => None
  | Some (w, rest) => match rest with
      | [] => None
      | o :: _ => let r := wrect w in
          Some (ensure_focus_visible e (with_windows c (rest ++ [set_rect (Rect (edge_i d w o rest) (rj r) (rw r) (rh r)) w])))
      end end end.

Definition send_to_edge e c d p : option canvas :=
  match focus c with None => Some c | Some f =>
  if (length (windows c) <? 2)%nat || negb (match d with Left | Right => true | _ => false end) then Some c else
  match window_of f (windows c), without f (windows c) with
  | Some w, o :: rest => let r := wrect w in
      match placed_by (windows c) [(f, Rect (edge_i d w o rest) (rj r) (rw r) (rh r))] p with
      | Some ws => Some (ensure_focus_visible e (with_windows c ws))
      | None => send_push e c d end
  | _, _ => send_push e c d end end.

(* move_by: one tile along d, the others where the answer p puts them; the fallback (move_by_push)
   pushes along d and leaves the gap behind open. *)
Definition move_by_push e c i d : option canvas :=
  match window_of i (windows c) with None => Some c | Some w =>
  match push d i (update i (set_rect (shifted (wrect w) (fst (delta d)) (snd (delta d)))) (windows c)) with
  | None => None
  | Some ws => Some (ensure_focus_visible e (with_windows c ws)) end end.

Definition move_by e c i d p : option canvas :=
  match window_of i (windows c) with None => Some c | Some w =>
  match placed_by (windows c) [(i, shifted (wrect w) (fst (delta d)) (snd (delta d)))] p with
  | Some ws => Some (ensure_focus_visible e (with_windows c ws))
  | None => move_by_push e c i d end end.

(* move_to: nothing when the target is the window's own rectangle; else the answer p, or the
   fallback (moved_to: take, then push right); when the window was a bridge, gather's answer g. *)
Definition moved_to i x y ws := match take i ws with None => Some ws | Some (w, rest) =>
  push Right i (rest ++ [set_rect (Rect x y (rw (wrect w)) (rh (wrect w))) w]) end.
Definition move_to e c i x y p g :=
  match rect_of i (windows c) with None => Some c | Some r =>
  let t := Rect x y (rw r) (rh r) in
  if rect_eqb t r then Some c else
  match match placed_by (windows c) [(i, t)] p with Some ws => Some ws | None => moved_to i x y (windows c) end with
  | None => None
  | Some ws => Some (ensure_focus_visible e (with_windows c (if is_bridge (windows c) i then gathered ws g else ws))) end end.

(* gather_islands: gather's answer g for every window, kept as gathered keeps it (one rectangle per
   window, every span kept, no two overlapping), then the focus shown; the undo step it records is
   not modelled. *)
Definition gather_islands e c g := ensure_focus_visible e (with_windows c (gathered (windows c) g)).

Definition enforce_all e a ws :=
  fold_left (fun acc i => match acc with Some ws => enforce_min e a i ws | None => None end) (ids ws) (Some ws).
(* centred_view along one axis: the view that keeps the canvas point under the middle of a screen
   sc in place when the tile goes from t0 to t1, (v + sc / 2) / t0 * t1 - sc / 2, floored. *)
Definition centred t0 t1 sc v := ((2 * v + sc) * t1 - sc * t0) / (2 * t0).
Definition rescaled e c z := (centred (tile_w e (zoom c)) (tile_w e z) (fst (screen c)) (fst (view c)),
  centred (tile_h e (zoom c)) (tile_h e z) (snd (screen c)) (snd (view c))).
(* zoom_by moves the camera: refused only when the clamped zoom is the current one; no window
   changes; the view keeps the canvas point under the middle of the screen, then shows the focus. *)
Definition zoom_by e c d :=
  let z := Z.max (zoom_min e) (Z.min (zoom_max e) (zoom c + d)) in
  if z =? zoom c then c else ensure_focus_visible e (Canvas (windows c) (focus c) z (rescaled e c z) (screen c) (recent c)).

Definition set_min e c i mn a : option canvas :=
  match window_of i (windows c) with None => Some c | Some _ =>
  match enforce_min e a i (update i (fun w => Window (wid w) (wrect w) mn) (windows c)) with
  | None => None
  | Some ws => Some (ensure_focus_visible e (with_windows c ws)) end end.
(* focus_id; focus_dir and focus_view_center focus some window through it. *)
Definition focus_id e c i := if mem i (ids (windows c)) then ensure_focus_visible e (set_focus c i) else c.
Definition set_screen e c s := ensure_focus_visible e (Canvas (windows c) (focus c) (zoom c) (view c) s (recent c)).
(* set_config: the zoom clamped into the new bounds, every minimum enforced at the new tile, the
   focus shown; the view is not rescaled. Every later operation runs under the new env. *)
Definition set_config e a c : option canvas :=
  let z := Z.max (zoom_min e) (Z.min (zoom_max e) (zoom c)) in
  match enforce_all e a (windows c) with
  | None => None
  | Some ws => Some (ensure_focus_visible e (Canvas ws (focus c) z (view c) (screen c) (recent c))) end.
(* rename: refused when the new id is taken or the window is missing; the focus and recent follow. *)
Definition rename c i j :=
  if mem j (ids (windows c)) then c else
  match window_of i (windows c) with None => c | Some _ =>
  Canvas (update i (fun w => Window j (wrect w) (wmin w)) (windows c))
    (match focus c with Some f => Some (if Nat.eqb f i then j else f) | None => None end)
    (zoom c) (view c) (screen c) (map (fun r => if Nat.eqb r i then j else r) (recent c)) end.

(* undo and redo restore a snapshot l: refused (nothing changes) unless it gives every window a
   rectangle (the first entry for its id, as many entries as windows) at a positive span with no two
   overlapping; then every minimum is enforced, as set_config does, and the focus shown. *)
Definition restored ws (l : list (nat * rect)) :=
  let ws' := map (fun w => match target_of (wid w) l with Some r => set_rect r w | None => w end) ws in
  if (length l =? length ws)%nat && forallb (fun w => match target_of (wid w) l with Some _ => true | None => false end) ws
     && forallb (fun w => (1 <=? rw (wrect w)) && (1 <=? rh (wrect w))) ws' && pairwise (map wrect ws')
  then Some ws' else None.
Definition restore e c l a := match restored (windows c) l with None => Some c | Some ws =>
  match enforce_all e a ws with None => None | Some ws' => Some (ensure_focus_visible e (with_windows c ws')) end end.

Inductive op :=
(* p and g are the engine's and gather's answers, a its answers for each minimum's growth, l the
   snapshot undo or redo restores. *)
  | Add (i : nat) (m : Z * Z) (want : option (Z * Z)) | MoveBy (i : nat) (d : dir) (p : option (list rect))
  | Remove (i : nat) (g : option (list rect)) | FocusId (i : nat) | Resize (i : nat) (ed : edge) (d : Z) (p : option (list rect))
  | Swap (n : nat) (d : dir) (p : option (list rect)) | SendToEdge (d : dir) (p : option (list rect))
  | MoveTo (i : nat) (x y : Z) (p g : option (list rect)) | ZoomBy (d : Z)
  | SetMin (i : nat) (m : Z * Z) (a : answers) | SetScreen (s : Z * Z) | SetConfig (e : env) (a : answers) | Rename (i j : nat)
  | AddAt (i : nat) (m : Z * Z) (at_ : rect) | Restore (l : list (nat * rect)) (a : answers)
  | GatherIslands (g : option (list rect)).
Definition step e c o : option canvas := match o with
  | Add i m w => add e c i m w | MoveBy i d p => move_by e c i d p | Remove i g => remove e c i g | FocusId i => Some (focus_id e c i)
  | Resize i ed d p => option_map snd (resize e c i ed d p) | Swap n d p => swap e c n d p
  | SendToEdge d p => send_to_edge e c d p | MoveTo i x y p g => move_to e c i x y p g | ZoomBy d => Some (zoom_by e c d)
  | SetMin i m a => set_min e c i m a | SetScreen s => Some (set_screen e c s)
  | SetConfig e' a => set_config e' a c | Rename i j => Some (rename c i j) | AddAt i m at_ => add_at e c i m at_
  | Restore l a => restore e c l a | GatherIslands g => Some (gather_islands e c g) end.
Definition env_after e o := match o with SetConfig e' _ => e' | _ => e end.
Fixpoint run e c os := match os with
  | [] => Some c | o :: os' => match step e c o with None => None | Some c' => run (env_after e o) c' os' end end.

(* A Config at a scale of 1/10000 px: tw x th px tiles at 1:1 (zf 0 = 10000), zoom bounds lo..hi,
   Config::default's 0.8 step, 0.8^z = 4^z/5^z being exact for zoom -2..4; need is ceil(min / tile). *)
Definition zf z := 5 ^ (4 - z) * 4 ^ (z + 2).
Definition ceil_div a b := - (- a / b).
Definition config tw th lo hi := Env (fun z => tw * zf z) (fun z => th * zf z)
  (fun m => (Z.max 1 (ceil_div (fst m) tw), Z.max 1 (ceil_div (snd m) th))) 0 lo hi.
Definition shipped := config 720 465 (-2) 4.
Definition canvas_new w h := Canvas [] None 0 (0, 0) (w * 10000, h * 10000) [].
Definition rect_after w h os i := match run shipped (canvas_new w h) os with
  | Some c => rect_of i (windows c) | None => None end.

(* The fixed scenarios of tests/replay.rs. *)
Example replays_the_fixed_scenarios :
  fits shipped 0 (screen (canvas_new 1440 930)) 2 2 = true /\ fits shipped 0 (screen (canvas_new 1440 930)) 3 1 = false /\
  rect_after 1440 930 [Add 1 (100, 100) None; Add 2 (100, 100) None] 2 = Some (Rect 1 0 1 1) /\
  rect_after 1440 930 [Add 1 (100, 100) None; Add 2 (100, 100) None; Resize 2 ELeft 1 (Some [Rect (-1) 0 1 1; Rect 0 0 2 1])] 2 = Some (Rect 0 0 2 1) /\
  rect_after 1440 930 [Add 1 (100, 100) None; Add 2 (100, 100) None; Resize 2 ELeft 1 (Some [Rect (-1) 0 1 1; Rect 0 0 2 1])] 1 = Some (Rect (-1) 0 1 1) /\
  rect_after 1440 930 [Add 1 (100, 100) None; Resize 1 ERight 1 (Some [Rect 0 0 2 1]); Resize 1 ERight 1 None] 1 = Some (Rect 0 0 2 1) /\
  option_map (fun c => (rect_of 3 (windows c), focus c))
    (run shipped (canvas_new 4096 1122) [Add 1 (100, 100) None; Add 2 (100, 100) None; Add 3 (100, 100) None; Remove 2 None])
    = Some (Some (Rect 1 0 1 1), Some 3%nat) /\
  option_map (fun c => (view c, view (focus_id shipped c 3), view (focus_id shipped c 2)))
    (run shipped (canvas_new 1440 930) (map (fun i => AddAt i (100, 100) (Rect (Z.of_nat i - 1) 0 1 1)) [1; 2; 3; 4]%nat))
    = Some ((1440 * 10000, 0), (1440 * 10000, 0), (720 * 10000, 0)) /\
  rect_after 4096 1122 [Add 1 (100, 100) None; Resize 1 ERight 1 (Some [Rect 0 0 2 1]); Add 2 (100, 100) None; FocusId 1; Resize 1 ERight (-1) None] 2
    = Some (Rect 1 0 1 1) /\
  rect_after 4096 1122 [Add 1 (100, 100) None; Resize 1 EBottom 1 (Some [Rect 0 0 1 2]); Add 2 (100, 100) None;
    Resize 1 ERight 1 (Some [Rect 0 0 2 2; Rect 2 0 1 1]); Resize 1 ETop (-1) None] 2
    = Some (Rect 2 0 1 1).
Proof. vm_compute. repeat split. Qed.

(* Config::default meets the model's hypotheses. *)
Example shipped_is_good : good shipped /\ good (Env (fun _ => 1) (fun _ => 1) (fun _ => (1, 1)) 0 0 0).
Proof. unfold good, shipped, config; simpl. repeat split; try lia; intros; lia. Qed.

(* The model's constants and edges sit where the code's do. *)
Example binds_the_boundaries :
  canvas_new 1440 930 = Canvas [] None 0 (0, 0) (14400000, 9300000) [] /\
  need shipped (720, 465) = (1, 1) /\ need shipped (721, 466) = (2, 2) /\
  fits shipped 0 (14400000, 9300000) 2 1 = true /\ fits shipped 0 (14399999, 9300000) 2 1 = false /\
  zoom (zoom_by shipped (canvas_new 1440 930) 9) = 4 /\ zoom (zoom_by shipped (canvas_new 1440 930) (-9)) = -2 /\
  zoom_by shipped (zoom_by shipped (canvas_new 1440 930) 9) 1 = zoom_by shipped (canvas_new 1440 930) 9 /\
  view (zoom_by shipped (Canvas [] (Some 7%nat) 0 (7200000, 4650000) (14400000, 9300000) []) 1) = (4320000, 2790000) /\
  rect_after 1440 930 [Add 1 (100, 100) None; Add 2 (100, 100) None; SendToEdge Left (Some [Rect 0 0 1 1; Rect (-1) 0 1 1])] 2 = Some (Rect (-1) 0 1 1) /\
  map (fun '(s, i) => let ws := [Window 1 (Rect 1 0 1 1) (0, 0); Window 2 (Rect (-1) 0 1 1) (0, 0); Window 3 (Rect 0 1 1 1) (0, 0);
     Window 4 (Rect 0 (-1) 1 1) (0, 0); Window 5 (Rect 1 1 1 1) (0, 0); Window 6 (Rect 3 0 1 1) (0, 0);
     Window 7 (Rect (-3) 0 1 1) (0, 0); Window 8 (Rect 0 (-3) 1 1) (0, 0)] in
     (chain (Rect 0 0 1 1) s ws, rect_of i (fill (Rect 0 0 1 1) s ws))) [(Right, 1%nat); (Left, 2%nat); (Down, 3%nat); (Up, 4%nat)]
    = [([1%nat], Some (Rect 0 0 1 1)); ([2%nat], Some (Rect 0 0 1 1)); ([3%nat], Some (Rect 0 0 1 1)); ([4%nat], Some (Rect 0 0 1 1))] /\
  nearest (Rect 0 0 1 1) [Window 2 (Rect 1 0 3 1) (0, 0); Window 1 (Rect (-2) 0 1 1) (0, 0)] = Some 1%nat /\
  nearest (Rect 0 0 1 1) [Window 2 (Rect 0 1 1 3) (0, 0); Window 1 (Rect 0 (-2) 1 1) (0, 0)] = Some 1%nat /\
  nearest (Rect 0 0 1 1) [Window 1 (Rect 5 0 1 1) (0, 0); Window 2 (Rect 1 0 1 1) (0, 0)] = Some 2%nat /\
  nearest (Rect 0 0 1 1) [Window 1 (Rect (-2) 0 1 1) (0, 0); Window 2 (Rect 4 0 1 1) (0, 0)] = Some 1%nat /\
  nearest (Rect 0 0 1 1) [Window 1 (Rect 0 (-2) 1 1) (0, 0); Window 2 (Rect 0 4 1 1) (0, 0)] = Some 1%nat.
Proof. vm_compute. repeat split. Qed.

(* set_config and rename at their edges, as tests/replay.rs set_config_clamps_the_zoom_and_enforces_minimums
   and rename_moves_the_id_and_the_focus check them: the clamp from above and below, minimums at a
   new tile width and a later add under it, rename refused onto a taken id or from a missing one. *)
Definition layout c os := option_map (fun c => (map (fun w => (wid w, wrect w)) (windows c), focus c, zoom c))
  (run shipped c os).
Example binds_set_config_and_rename : let laptop := canvas_new 1440 930 in
  map (fun '(lo, hi) => option_map zoom (set_config (config 720 465 lo hi) pushes laptop)) [(1, 4); (-2, -1); (-2, 4)]
    = [Some 1; Some (-1); Some 0] /\
  layout laptop [Add 1 (700, 100) None; Add 2 (100, 100) None; SetConfig (config 360 465 (-2) 4) pushes; Add 3 (720, 465) None]
    = Some ([(1%nat, Rect 0 0 2 1); (2%nat, Rect 2 0 1 1); (3%nat, Rect 2 1 2 1)], Some 3%nat, 0) /\
  map (fun o => layout laptop [Add 1 (100, 100) None; Add 2 (100, 100) None; o]) [Rename 1 2; Rename 5 6; Rename 2 7; Rename 1 7]
    = [Some ([(1%nat, Rect 0 0 1 1); (2%nat, Rect 1 0 1 1)], Some 2%nat, 0);
       Some ([(1%nat, Rect 0 0 1 1); (2%nat, Rect 1 0 1 1)], Some 2%nat, 0);
       Some ([(1%nat, Rect 0 0 1 1); (7%nat, Rect 1 0 1 1)], Some 7%nat, 0);
       Some ([(7%nat, Rect 0 0 1 1); (2%nat, Rect 1 0 1 1)], Some 2%nat, 0)].
Proof. vm_compute. repeat split. Qed.

(* Canvas v2 as tests/replay.rs checks it on 2560x1440 at CONFIGS[3] (240x155 tiles): move_by
   takes the engine's layout (2 takes the tile 1 left), undo and redo restore the snapshots, add
   takes the nearest free spot the view scrolls least to, the corners after the sides, its wanted
   span, and closing the focus returns to the last used window. *)
Definition v2 e c os := option_map (fun c => (map (fun w => (wid w, wrect w)) (windows c), focus c, recent c)) (run e c os).
Example binds_canvas_v2 : let small := config 240 155 (-2) 7 in
  let rects os := option_map (fun '(ws, _, _) => map snd ws) (v2 small (canvas_new 2560 1440) os) in
  let three := [Add 1 (100, 100) None; Add 2 (100, 100) None; Add 3 (100, 100) None] in
  let beside n := flat_map (fun i => [FocusId 1; Add i (100, 100) None]) (seq 1 n) in
  let s0 := [Rect 0 0 1 1; Rect 1 0 1 1; Rect 2 0 1 1] in let s1 := [Rect 1 0 1 1; Rect 0 0 1 1; Rect 2 0 1 1] in
  let snap l := combine [1; 2; 3]%nat l in
  let moved := three ++ [FocusId 1; MoveBy 1 Right (Some s1); MoveBy 1 Down (Some [Rect 1 1 1 1; Rect 0 0 1 1; Rect 2 0 1 1])] in
  rects (three ++ [FocusId 1; MoveBy 1 Right (Some s1)]) = Some s1 /\
  rects (moved ++ [MoveBy 9 Left None]) = Some [Rect 1 1 1 1; Rect 0 0 1 1; Rect 2 0 1 1] /\
  rects (moved ++ [Restore (snap s1) pushes; Restore (snap s0) pushes]) = Some s0 /\ rects (moved ++ [Restore (snap s1) pushes; Restore (snap s0) pushes; Restore (snap s1) pushes]) = Some s1 /\
  rects (beside 7%nat) = Some [Rect 0 0 1 1; Rect 1 0 1 1; Rect 0 1 1 1; Rect 0 (-1) 1 1; Rect (-1) 0 1 1; Rect 1 (-1) 1 1; Rect 1 1 1 1] /\
  rects (beside 5%nat ++ [MoveTo 2 2 0 (Some [Rect 0 0 1 1; Rect 2 0 1 1; Rect 0 1 1 1; Rect 0 (-1) 1 1; Rect (-1) 0 1 1]) None;
      FocusId 1; Add 6 (100, 100) (Some (2, 1))])
    = Some [Rect 0 0 1 1; Rect 2 0 1 1; Rect 0 1 1 1; Rect 0 (-1) 1 1; Rect (-1) 0 1 1; Rect 1 (-1) 2 1] /\
  rects [Add 1 (380, 145) (Some (7, 7)); Add 2 (380, 145) None] = Some [Rect 0 0 7 7; Rect 7 0 2 1] /\
  open_span small (25600000, 14400000) (0, 0) (Some (50, 50)) = (10, 9) /\
  open_span small (25600000, 14400000) (1000, 100) (Some (1, 0)) = (5, 1) /\
  map (fun n => option_map (fun '(_, f, r) => (f, r)) (v2 small (canvas_new 2560 1440) (firstn n (three ++ [FocusId 1; FocusId 3; Remove 3 None; Rename 2 9; Remove 1 None; Remove 9 None]))))
    [5; 6; 7; 8; 9]%nat = map Some [(Some 3, [3; 1; 2]); (Some 1, [1; 2]); (Some 1, [1; 9]); (Some 9, [9]); (None, [])]%nat.
Proof. vm_compute. repeat split. Qed.

(* add_at as tests/replay.rs add_at_puts_a_window_back_on_its_tiles_or_the_nearest_free_ones
   checks it, then spot_near's rings past a 3x3 window on 2560x1440: the nearest first, then the
   upper, then the left. *)
Example binds_add_at : let rects c os := option_map (fun '(ws, f, r) => (map snd ws, f, r)) (v2 shipped c os) in
  rects (canvas_new 1440 930) [AddAt 1 (0, 0) (Rect 3 1 2 1); AddAt 2 (0, 0) (Rect (-2) 0 1 2);
    AddAt 3 (0, 0) (Rect 3 1 2 1); AddAt 4 (0, 0) (Rect 10 5 3 1); AddAt 5 (800, 0) (Rect (-2) 0 1 1)]
    = Some ([Rect 3 1 2 1; Rect (-2) 0 1 2; Rect 3 0 2 1; Rect 10 5 2 1; Rect (-2) (-1) 2 1], Some 5%nat, [5; 4; 3; 2; 1]%nat) /\
  rects (canvas_new 2560 1440) [AddAt 1 (0, 0) (Rect (-1) (-1) 3 3); AddAt 2 (0, 0) (Rect 0 0 1 1); AddAt 3 (0, 0) (Rect 0 0 1 1)]
    = Some ([Rect (-1) (-1) 3 3; Rect 0 (-2) 1 1; Rect (-2) 0 1 1], Some 3%nat, [3; 2; 1]%nat).
Proof. vm_compute. repeat split. Qed.

(* The camera as tests/replay.rs checks it: magnified past 1:1 the focus keeps its tiles and is
   shown from its top-left, and the view scrolls the least to the next focus; a grow stops where
   the span fits at 1:1 and a minimum takes the same tiles at every zoom; a wanted span is cut at
   1:1 at any zoom, and ghostty keeps its two small tiles through zooms in and out. *)
Example binds_the_camera :
  option_map (fun c => (rect_of 1 (windows c), view c, view (focus_id shipped c 2), view (focus_id shipped (focus_id shipped c 2) 1)))
    (run shipped (canvas_new 1440 930) [Add 1 (1400, 900) None; AddAt 2 (100, 100) (Rect 2 0 1 1); FocusId 1; ZoomBy (-2)])
    = Some (Some (Rect 0 0 2 2), (0, 0), (3 * 11250000 - 14400000, 0), (0, 0)) /\ fits shipped (-2) (14400000, 9300000) 2 2 = false /\
  map (fun z => rect_after 1440 930 [ZoomBy z; Add 1 (100, 100) None; Resize 1 ERight 1 (Some [Rect 0 0 2 1]); Resize 1 ERight 1 None;
    Resize 1 EBottom 1 (Some [Rect 0 0 2 2]); Resize 1 EBottom 1 None] 1) [-2; -1; 0; 1; 4] = repeat (Some (Rect 0 0 2 2)) 5 /\
  map (fun z => rect_after 1440 930 [ZoomBy z; Add 1 (800, 500) None] 1) [-2; 0; 4] = repeat (Some (Rect 0 0 2 2)) 3 /\
  map (fun z => v2 (config 240 155 (-2) 7) (canvas_new 2560 1440) [ZoomBy z; Add 1 (0, 0) (Some (50, 50))]) [-2; 3]
    = map (fun z => Some ([(1%nat, Rect 0 0 10 9)], Some 1%nat, [1%nat])) [-2; 3] /\
  option_map (fun c => (rect_of 1 (windows c), zoom c))
    (run (config 240 155 (-2) 7) (canvas_new 2560 1440) [ZoomBy 3; Add 1 (367, 132) None; ZoomBy (-5)]) = Some (Some (Rect 0 0 2 1), -2).
Proof. vm_compute. repeat split. Qed.

(* The middle of the screen through zooms, as tests/replay.rs a_zoom_keeps_the_middle_of_the_screen
   and a_laptop_canvas_pulls_back_about_the_middle check: pulled back, the view keeps (1280, 720) on
   2560x1440 and (714, 460) on a 1428x920 canvas with 119x115 tiles where they are; closer again
   it returns; coming closer first scrolls the least to show the focus, and pulling back keeps
   that scroll, scaled. *)
Fixpoint views e c os := match os with [] => [] | o :: os' =>
  match step e c o with None => [] | Some c' => view c' :: views (env_after e o) c' os' end end.
Example binds_the_middle :
  views shipped (canvas_new 2560 1440) [Add 1 (100, 100) None; Add 2 (100, 100) None; Add 3 (100, 100) None;
    ZoomBy 1; ZoomBy 1; ZoomBy (-2); ZoomBy (-1); ZoomBy 1]
    = [(0, 0); (0, 0); (0, 0); (-2560000, -1440000); (-4608000, -2592000); (0, 0); (3200000, 0); (0, -1440000)] /\
  views (config 119 115 (-2) 4) (canvas_new 1428 920) [Add 1 (476, 460) None; Add 2 (476, 460) None; Add 3 (476, 460) None;
    ZoomBy 2; ZoomBy (-2); ZoomBy (-1); ZoomBy 1]
    = [(0, 0); (0, 0); (0, 0); (-2570400, -1656000); (0, 0); (3570000, 0); (1428000, -920000)].
Proof. vm_compute. repeat split. Qed.

(* C71: beside a focus the view shows whole, no side spot needs a scroll, so side decides: a new
   window opens right, then below, left, above, then at the corners, right ones first
   (tests/replay.rs a_new_window_opens_right_below_left_above). Undo refuses a snapshot that names
   a window the canvas does not hold in place of one it does (lib.rs
   restore_refuses_a_partial_overlapping_or_empty_snapshot). *)
Example opens_right_below_left_above :
  let around n := flat_map (fun k => [FocusId 1; Add k (100, 100) None]) (seq 2 n) in
  option_map (fun c => (map wrect (windows c), view c))
    (run shipped (canvas_new 2560 1440) (AddAt 1 (100, 100) (Rect 1 1 1 1) :: around 6%nat))
    = Some ([Rect 1 1 1 1; Rect 2 1 1 1; Rect 1 2 1 1; Rect 0 1 1 1; Rect 1 0 1 1; Rect 2 0 1 1; Rect 2 2 1 1], (0, 0)) /\
  restored [Window 1 (Rect 0 0 1 1) (100, 100); Window 2 (Rect 1 0 1 1) (100, 100)]
    [(1%nat, Rect 0 0 1 1); (3%nat, Rect 5 0 1 1)] = None.
Proof. vm_compute. auto. Qed.
