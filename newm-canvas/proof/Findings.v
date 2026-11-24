From Stdlib Require Import ZArith List Bool Lia.
Import ListNotations.
From NewmCanvas Require Import Canvas.
Open Scope Z_scope.

(* F01, repaired. push before 5541f892 was a queue: each popped pusher moved every other window
   that overlapped it past it and queued them (its fuel never runs out here). *)
Definition pushed m p pr w := negb (Nat.eqb (wid w) m) && negb (Nat.eqb (wid w) p) && overlaps (wrect w) pr.
Fixpoint queue d m (pops : nat) (q : list nat) ws := match q, pops with
  | p :: q', S pops' => match rect_of p ws with
      | None => queue d m pops' q' ws
      | Some pr => queue d m pops' (q' ++ map wid (filter (pushed m p pr) ws))
          (map (fun w => if pushed m p pr w then set_rect (past d pr (wrect w)) w else w) ws) end
  | _, _ => ws end.

(* 2 lies left of 1 in one row and move_to drops 3 on both (dropped): the queue moved both to 3's
   right edge in vector order, then 1 pushed 2 past it. The sweep keeps 2 before 1. *)
Definition dropped := [Window 1 (Rect 1 0 1 1) (100, 100); Window 2 (Rect 0 0 1 1) (100, 100); Window 3 (Rect 0 0 2 1) (100, 100)].
Example move_to_reverses_the_windows_it_lands_on :
  let crossing := [AddAt 1 (100, 100) (Rect 1 0 1 1); AddAt 2 (100, 100) (Rect 0 0 1 1); AddAt 3 (100, 100) (Rect 0 1 1 1);
    Resize 3 ERight 1 None] in
  option_map (fun c => option_map (fun '(w, rest) => rest ++ [set_rect (Rect 0 0 (rw (wrect w)) (rh (wrect w))) w])
    (take 3 (windows c))) (run shipped (canvas_new 1440 930) crossing) = Some (Some dropped) /\
  queue Right 3 13 [3%nat] dropped
  = [Window 1 (Rect 2 0 1 1) (100, 100); Window 2 (Rect 3 0 1 1) (100, 100); Window 3 (Rect 0 0 2 1) (100, 100)].
Proof. vm_compute. auto. Qed.

Example repaired_push_keeps_their_order : push Right 3 dropped
  = Some [Window 1 (Rect 3 0 1 1) (100, 100); Window 2 (Rect 2 0 1 1) (100, 100); Window 3 (Rect 0 0 2 1) (100, 100)].
Proof. reflexivity. Qed.

(* F02, repaired by the camera (3c0b8d52). Before it a zoom gave every window the tiles its
   minimum needed at the zoomed tile (enforce_all under the env whose 1:1 tile is zoom 1's,
   576x372) after checking the focus at its old span: a 1400 px minimum's two 720 px tiles passed
   as two 576 px tiles, then became three, 1728 px on 1440 px. *)
Example zoom_out_leaves_the_focus_wider_than_the_screen :
  match run shipped (canvas_new 1440 930) [Add 1 (1400, 100) None] with
  | Some c => fits shipped 1 (screen c) 2 1 = true /\
      option_map (rect_of 1) (enforce_all (config 576 372 (-2) 4) pushes (windows c)) = Some (Some (Rect 0 0 3 1)) /\
      fits shipped 1 (screen c) 3 1 = false
  | None => False end.
Proof. vm_compute. auto. Qed.

(* Under the camera zoom 1 keeps the focus's two tiles, which fit and show whole. *)
Example camera_keeps_it_on_screen :
  match run shipped (canvas_new 1440 930) [Add 1 (1400, 100) None; ZoomBy 1] with
  | Some c => zoom c = 1 /\ rect_of 1 (windows c) = Some (Rect 0 0 2 1) /\ fits shipped 1 (screen c) 2 1 = true /\
      is_visible shipped c (Rect 0 0 2 1) = true
  | None => False end.
Proof. vm_compute. auto. Qed.

(* F03: a config whose bounds exclude the zoom moves it, which the crate doc says only zoom_by does. *)
Example set_config_moves_the_zoom : option_map zoom (set_config (config 720 465 1 4) pushes (canvas_new 1440 930)) = Some 1.
Proof. reflexivity. Qed.

(* F04, repaired by src/layout/canvas/moves.rs: a drag ended in a remove and an add, which opens the
   window at a new window's span; move_to keeps it (C47). A 2x2 window on 2560x1440: *)
Definition two_by_two := [Add 1 (100, 100) None; Resize 1 ERight 1 None; Resize 1 EBottom 1 None].
Example remove_then_add_is_not_a_move :
  option_map (fun c => rect_of 1 (windows c)) (run shipped (canvas_new 2560 1440) (two_by_two ++ [Remove 1 None; Add 1 (100, 100) None]))
    = Some (Some (Rect 0 0 1 1)) /\
  option_map (fun c => rect_of 1 (windows c)) (run shipped (canvas_new 2560 1440) (two_by_two ++ [MoveTo 1 1 0 (Some [Rect 1 0 2 2]) None]))
    = Some (Some (Rect 1 0 2 2)).
Proof. vm_compute. auto. Qed.

(* F05, repaired: swap traded rectangles (old_swap), so a 2x2 focus swapped with its 1x1 neighbour
   became 1x1 and the neighbour 2x2. Now the focus ends where the neighbour ended and both keep
   their spans (C47). *)
Definition old_swap f n ws := match rect_of f ws, rect_of n ws with
  | Some ra, Some rb => update n (set_rect ra) (update f (set_rect rb) ws) | _, _ => ws end.
Example swap_traded_spans :
  match run shipped (canvas_new 2560 1440) (two_by_two ++ [Add 2 (100, 100) None; FocusId 1]) with
  | Some c => (rect_of 1 (old_swap 1 2 (windows c)), rect_of 2 (old_swap 1 2 (windows c)))
        = (Some (Rect 2 0 1 1), Some (Rect 0 0 2 2)) /\
      option_map (fun c => (rect_of 1 (windows c), rect_of 2 (windows c))) (swap shipped c 2 Right (Some [Rect 1 0 2 2; Rect 0 0 1 1]))
        = Some (Some (Rect 1 0 2 2), Some (Rect 0 0 1 1))
  | None => False end.
Proof. vm_compute. auto. Qed.
(* F09, repaired: undo put a snapshot back as it was (old_restore), so a minimum raised since the
   snapshot left the window below its need; restore now enforces every minimum after it. *)
Definition old_restore c (l : list (nat * rect)) :=
  with_windows c (map (fun w => match target_of (wid w) l with Some r => set_rect r w | None => w end) (windows c)).
Example undo_left_a_window_below_its_minimum : let snap := [(1%nat, Rect 0 0 1 1); (2%nat, Rect 1 0 1 1)] in
  match run shipped (canvas_new 2560 1440) [Add 1 (100, 100) None; Add 2 (100, 100) None;
    MoveBy 2 Right (Some [Rect 0 0 1 1; Rect 2 0 1 1]); SetMin 2 (1400, 100) pushes] with
  | Some c => rect_of 2 (windows (old_restore c snap)) = Some (Rect 1 0 1 1) /\ need shipped (1400, 100) = (2, 1) /\
      option_map (fun c' => rect_of 2 (windows c')) (restore shipped c snap pushes) = Some (Some (Rect 1 0 2 1))
  | None => False end.
Proof. vm_compute. repeat split. Qed.
(* F10, repaired: send_to_edge's push fallback measured the edge before take's fill (old_send); the
   fill slid 2 into the tile the focus 1 left, and 1 sent left landed on 2. It measures it after. *)
Definition old_send d c := match focus c with None => c | Some f =>
  match window_of f (windows c), without f (windows c), take f (windows c) with
  | Some w, o :: rest, Some (w', rest') =>
      with_windows c (rest' ++ [set_rect (Rect (edge_i d w o rest) (rj (wrect w)) (rw (wrect w)) (rh (wrect w))) w'])
  | _, _, _ => c end end.
Example send_fallback_landed_on_a_window :
  let c := Canvas [Window 1 (Rect 0 0 1 1) (0, 0); Window 2 (Rect 1 0 1 1) (0, 0)] (Some 1%nat) 0 (0, 0) (14400000, 9300000) [1; 2]%nat in
  map wrect (windows (old_send Left c)) = [Rect 0 0 1 1; Rect 0 0 1 1] /\
  option_map (fun c => map wrect (windows c)) (send_to_edge shipped c Left None) = Some [Rect 0 0 1 1; Rect (-1) 0 1 1].
Proof. vm_compute. repeat split. Qed.

