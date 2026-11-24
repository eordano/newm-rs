(* src/place.rs Search::step, the faster candidate search, against the scans it replaced.
   1. The candidate grid: each rectangle adds its value over the candidates that overlap it by
      four corner updates of a difference array and one in-place 2D prefix sum, so each cell
      holds the sum over the rectangles that overlap its candidate: blocked, clear and pending
      are the old per-candidate scans (grid_sum, blocked_scan, clear_scan, pending_scan).
   2. The tiles term does not depend on the order the moved windows are listed in, so the
      search's DFS order sums to what window order sums to (fresh_terms_perm, fresh_terms_insert).
   3. A candidate shares no tile with a rectangle off the room, so the term against every moved
      window is the Rust's held + freed (fresh_room).
   Representation: one plane per slot of grid (the Rust keeps the three slots side by side), a
   plane being a function over (column, row) indices instead of the flat vector. *)
From Stdlib Require Import ZArith List Bool Lia Permutation.
Import ListNotations.
From NewmCanvas Require Import Canvas Index Cost.
From NewmCanvas Require Push Rules.
Open Scope Z_scope.

Ltac cases := repeat match goal with
  | |- context [Nat.eqb ?x ?y] => destruct (Nat.eqb_spec x y)
  | |- context [Nat.leb ?x ?y] => destruct (Nat.leb_spec x y)
  | |- context [Nat.ltb ?x ?y] => destruct (Nat.ltb_spec x y)
  | |- context [Z.eqb ?x ?y] => destruct (Z.eqb_spec x y)
  | |- context [Z.leb ?x ?y] => destruct (Z.leb_spec x y)
  | |- context [Z.ltb ?x ?y] => destruct (Z.ltb_spec x y)
  end; cbn [andb orb negb]; try lia.

(* ---------- 1. The candidate grid ---------- *)

(* One slot of grid, cell (a, b) being grid[a * (gh + 1) + b]; zero after grid.resize. *)
Definition plane := nat -> nat -> Z.
Definition zero : plane := fun _ _ => 0.
Definition bump (g : plane) a c v : plane := fun x y => if Nat.eqb x a && Nat.eqb y c then g x y + v else g x y.

(* cover's columns a..=b and rows c..=d of the candidates Rect i j o.w o.h over r. *)
Definition bounds (o : rect) i0 j0 (gw gh : nat) r :=
  (Z.max (ri r - rw o + 1) i0, Z.min (right r - 1) (i0 + Z.of_nat gw - 1),
   Z.max (rj r - rh o + 1) j0, Z.min (bottom r - 1) (j0 + Z.of_nat gh - 1)).
Definition in_cover o i0 j0 gw gh r i j :=
  let '(a, b, c, d) := bounds o i0 j0 gw gh r in (a <=? i) && (i <=? b) && (c <=? j) && (j <=? d).
Definition cover o i0 j0 gw gh (g : plane) (rv : rect * Z) : plane :=
  let '(r, v) := rv in
  let '(a, b, c, d) := bounds o i0 j0 gw gh r in
  if (b <? a) || (d <? c) then g else
  let a := Z.to_nat (a - i0) in let b := S (Z.to_nat (b - i0)) in
  let c := Z.to_nat (c - j0) in let d := S (Z.to_nat (d - j0)) in
  bump (bump (bump (bump g a c v) b c (- v)) a d (- v)) b d v.
Definition covers o i0 j0 gw gh (l : list (rect * Z)) := fold_left (cover o i0 j0 gw gh) l zero.

(* The in-place prefix pass: for a in 0..=gw, b in 0..=gh, grid[a][b] += left + up - corner. *)
Definition prev (n : nat) (f : nat -> Z) := match n with O => 0 | S n' => f n' end.
Definition cell (g : plane) a b : plane := fun x y =>
  if Nat.eqb x a && Nat.eqb y b
  then g a b + prev a (fun a' => g a' b) + prev b (g a) - prev a (fun a' => prev b (g a'))
  else g x y.
Fixpoint row (g : plane) a (n : nat) : plane := match n with O => g | S n' => cell (row g a n') a n' end.
Fixpoint prefix (g : plane) (m n : nat) : plane := match m with O => g | S m' => row (prefix g m' n) m' n end.
Definition table o i0 j0 gw gh l := prefix (covers o i0 j0 gw gh l) (S gw) (S gh).

(* The values the pass leaves: P[a][b] = D[a][b] + P[a-1][b] + P[a][b-1] - P[a-1][b-1]. *)
Fixpoint prow (g : plane) (up : nat -> Z) a b := match b with
  | O => g a O + up O
  | S b' => g a b + up b + prow g up a b' - up b' end.
Fixpoint pre (g : plane) a : nat -> Z := match a with O => prow g (fun _ => 0) O | S a' => prow g (pre g a') a end.

Lemma pre_eq g a b :
  pre g a b = g a b + prev a (fun a' => pre g a' b) + prev b (pre g a) - prev a (fun a' => prev b (pre g a')).
Proof. destruct a, b; cbn [pre prow prev]; lia. Qed.

Lemma row_spec g0 g a n k : (k <= n)%nat ->
  (forall x y, g x y = if Nat.ltb x a && Nat.ltb y n then pre g0 x y else g0 x y) ->
  forall x y, row g a k x y = if Nat.ltb x a && Nat.ltb y n || Nat.eqb x a && Nat.ltb y k then pre g0 x y else g0 x y.
Proof.
  intros Hk G. induction k as [|k IH]; intros x y; cbn [row]; [rewrite G; cases|].
  unfold cell; cbv beta. destruct (Nat.eqb_spec x a) as [->|], (Nat.eqb_spec y k) as [->|]; cbn [andb];
    [|rewrite IH by lia; cases..].
  rewrite (pre_eq g0 a k). destruct a, k; cbn [prev]; rewrite ?IH by lia; cases.
Qed.

Lemma prefix_spec g0 m n x y : prefix g0 m n x y = if Nat.ltb x m && Nat.ltb y n then pre g0 x y else g0 x y.
Proof.
  revert x y; induction m as [|m IH]; intros x y; cbn [prefix]; [cases|].
  rewrite (row_spec g0 (prefix g0 m n) m n n (le_n n) IH). cases.
Qed.

Lemma pre_bump g x0 y0 v a b : pre (bump g x0 y0 v) a b = pre g a b + (if Nat.leb x0 a && Nat.leb y0 b then v else 0).
Proof.
  revert b; induction a as [|a IHa]; intros b; induction b as [|b IHb];
    rewrite (pre_eq (bump g x0 y0 v)), (pre_eq g); cbn [prev]; rewrite ?IHa, ?IHb; unfold bump; cbv beta; cases.
Qed.

Lemma pre_zero a b : pre zero a b = 0.
Proof. revert b; induction a as [|a IHa]; intros b; induction b as [|b IHb]; rewrite pre_eq; cbn [prev]; rewrite ?IHa, ?IHb; unfold zero; lia. Qed.

(* Candidate (i, j) is in r's range exactly when it overlaps r, whatever the spans. *)
Theorem cover_range o i0 j0 gw gh r i j : i0 <= i < i0 + Z.of_nat gw -> j0 <= j < j0 + Z.of_nat gh ->
  in_cover o i0 j0 gw gh r i j = overlaps (Rect i j (rw o) (rh o)) r.
Proof.
  intros Hi Hj. unfold in_cover, bounds, overlaps, rows_overlap, cols_overlap, right, bottom; cbn [ri rj rw rh].
  apply eq_true_iff_eq. rewrite !andb_true_iff, !Z.leb_le, !Z.ltb_lt. lia.
Qed.

Lemma pre_cover o i0 j0 gw gh g r v a b : (a < gw)%nat -> (b < gh)%nat ->
  pre (cover o i0 j0 gw gh g (r, v)) a b =
  pre g a b + (if in_cover o i0 j0 gw gh r (i0 + Z.of_nat a) (j0 + Z.of_nat b) then v else 0).
Proof.
  intros Ha Hb. unfold cover, in_cover. destruct (bounds o i0 j0 gw gh r) as [[[A B] C] D] eqn:E.
  assert (i0 <= A /\ B <= i0 + Z.of_nat gw - 1 /\ j0 <= C /\ D <= j0 + Z.of_nat gh - 1)
    by (unfold bounds in E; injection E; lia).
  cbv beta iota zeta. destruct ((B <? A) || (D <? C)) eqn:G.
  - rewrite orb_true_iff, !Z.ltb_lt in G. cases.
  - rewrite orb_false_iff, !Z.ltb_ge in G. rewrite !pre_bump. cases.
Qed.

Lemma pre_covers o i0 j0 gw gh l g a b : (a < gw)%nat -> (b < gh)%nat ->
  pre (fold_left (cover o i0 j0 gw gh) l g) a b =
  pre g a b + sumf (fun p => if in_cover o i0 j0 gw gh (fst p) (i0 + Z.of_nat a) (j0 + Z.of_nat b) then snd p else 0) l.
Proof.
  intros Ha Hb. revert g; induction l as [|[r v] l IH]; intros g; cbn [fold_left sumf fst snd]; [lia|].
  rewrite IH, pre_cover by assumption. lia.
Qed.

(* After covering the list and the prefix pass, a candidate's cell holds the sum of the values of
   the rectangles that overlap the candidate. *)
Theorem grid_sum o i0 j0 gw gh l a b : (a < gw)%nat -> (b < gh)%nat ->
  table o i0 j0 gw gh l a b =
  sumf (fun p => if overlaps (Rect (i0 + Z.of_nat a) (j0 + Z.of_nat b) (rw o) (rh o)) (fst p) then snd p else 0) l.
Proof.
  intros Ha Hb. unfold table, covers. rewrite prefix_spec.
  replace (Nat.ltb a (S gw) && Nat.ltb b (S gh)) with true by (symmetry; apply andb_true_iff; split; apply Nat.ltb_lt; lia).
  rewrite pre_covers, pre_zero by assumption. rewrite Z.add_0_l. apply sumf_ext. intros p _.
  rewrite cover_range by lia. reflexivity.
Qed.

(* ---------- The step's three slots against the old scans ---------- *)

(* Search::step's grid for window o (REACH = 2), its cell (a, b)'s candidate, and its room. *)
Definition step_table o l :=
  table o (ri o - (rw o + 2)) (rj o - (rh o + 2)) (Z.to_nat (2 * (rw o + 2) + 1)) (Z.to_nat (2 * (rh o + 2) + 1)) l.
Definition on_grid o a b := (a < Z.to_nat (2 * (rw o + 2) + 1))%nat /\ (b < Z.to_nat (2 * (rh o + 2) + 1))%nat.
Definition cand o a b := candidate o (ri o - (rw o + 2) + Z.of_nat a) (rj o - (rh o + 2) + Z.of_nat b).
Definition step_room o := room o (rw o + 2) (rh o + 2).
Definition under before o := overlapping (index_new before) (step_room o).
Definition others (placed : nat -> bool) k p := negb (placed p) && negb (Nat.eqb p k).
Definition area r := rw r * rh r.

Lemma sumf_filter {A} (F : A -> Z) f l : sumf F (filter f l) = sumf (fun x => if f x then F x else 0) l.
Proof. induction l as [|x l IH]; cbn [filter sumf]; [reflexivity|]. destruct (f x); cbn [sumf]; lia. Qed.

Lemma ones {A} (f : A -> bool) l : 0 <= sumf (fun _ => 1) (filter f l) /\ existsb f l = (0 <? sumf (fun _ => 1) (filter f l)).
Proof.
  induction l as [|x l [N E]]; cbn [filter existsb]; [cbn; split; [lia|reflexivity]|].
  destruct (f x); cbn [sumf orb]; [split; [lia|symmetry; apply Z.ltb_lt; lia]|split; assumption].
Qed.

Lemma step_sum o (cur : nat -> rect) (val : nat -> Z) l a b : on_grid o a b ->
  step_table o (map (fun p => (cur p, val p)) l) a b = sumf val (filter (fun p => overlaps (cand o a b) (cur p)) l).
Proof. intros [Ha Hb]. unfold step_table. rewrite grid_sum by assumption. rewrite sumf_map, sumf_filter. reflexivity. Qed.

Lemma in_room o rx ry i j r : ri o - rx <= i <= ri o + rx -> rj o - ry <= j <= rj o + ry ->
  overlaps (candidate o i j) r = true -> overlaps r (room o rx ry) = true.
Proof.
  unfold overlaps, rows_overlap, cols_overlap, candidate, room, right, bottom; cbn [ri rj rw rh].
  rewrite !andb_true_iff, !Z.ltb_lt. intros. lia.
Qed.

Lemma cand_in o a b : on_grid o a b ->
  ri o - (rw o + 2) <= ri (cand o a b) <= ri o + (rw o + 2) /\ rj o - (rh o + 2) <= rj (cand o a b) <= rj o + (rh o + 2).
Proof. unfold on_grid, cand, candidate; cbn [ri rj]. lia. Qed.

(* The windows the index finds under the room, filtered as the step filters them, overlap the
   candidate exactly when the scan over every window finds them: a window not placed lies where
   it was, which is what the index holds. *)
Lemma under_scan o before (cur : nat -> rect) (F : nat -> bool) a b : on_grid o a b ->
  (forall p, F p = true -> cur p = nth p before d0) ->
  filter (fun p => overlaps (cand o a b) (cur p)) (filter F (under before o)) =
  filter (fun p => F p && overlaps (cand o a b) (cur p)) (seq 0 (length before)).
Proof.
  intros Hg H. destruct (cand_in o a b Hg) as [Hi Hj].
  unfold under. rewrite overlapping_scan, !filter_filter_and. apply filter_ext. intros p.
  destruct (F p) eqn:Fp, (overlaps (nth p before d0) (step_room o)) eqn:R, (overlaps (cand o a b) (cur p)) eqn:O;
    try reflexivity.
  rewrite (H p Fp) in O. apply (in_room o _ _ _ _ _ Hi Hj) in O. unfold step_room in R. congruence.
Qed.

(* Slot 0, every placed window adding 1: blocked > 0 is the old any(placed && overlaps). *)
Theorem blocked_scan o (cur : nat -> rect) placed n a b : on_grid o a b ->
  (0 <? step_table o (map (fun p => (cur p, 1)) (filter placed (seq 0 n))) a b) =
  existsb (fun p => placed p && overlaps (cand o a b) (cur p)) (seq 0 n).
Proof. intros Hg. rewrite step_sum, filter_filter_and by exact Hg. exact (eq_sym (proj2 (ones _ _))). Qed.

(* Slot 1, every window under the room not placed but k adding 1: others_under == 0 is "no
   window not placed but k overlaps the candidate" over every window. *)
Theorem clear_scan o before (cur : nat -> rect) placed k a b : on_grid o a b ->
  (forall p, placed p = false -> cur p = nth p before d0) ->
  (step_table o (map (fun p => (cur p, 1)) (filter (others placed k) (under before o))) a b =? 0) =
  negb (existsb (fun p => others placed k p && overlaps (cand o a b) (cur p)) (seq 0 (length before))).
Proof.
  intros Hg H. rewrite step_sum, under_scan by (exact Hg || (intros p Hp; apply H; unfold others in Hp;
    destruct (placed p); [discriminate|reflexivity])).
  destruct (ones (fun p => others placed k p && overlaps (cand o a b) (cur p)) (seq 0 (length before))) as [N ->].
  cases.
Qed.

(* Slot 2, the same windows adding their areas: pending is the old filter-and-sum of areas. *)
Theorem pending_scan o before (cur : nat -> rect) placed k a b : on_grid o a b ->
  (forall p, placed p = false -> cur p = nth p before d0) ->
  step_table o (map (fun p => (cur p, area (cur p))) (filter (others placed k) (under before o))) a b =
  sumf (fun p => area (cur p)) (filter (fun p => others placed k p && overlaps (cand o a b) (cur p)) (seq 0 (length before))).
Proof.
  intros Hg H. rewrite step_sum, under_scan by (exact Hg || (intros p Hp; apply H; unfold others in Hp;
    destruct (placed p); [discriminate|reflexivity])).
  reflexivity.
Qed.

(* ---------- 2. The order of the moved windows ---------- *)

Definition zz_dec : forall x y : Z * Z, {x = y} + {x <> y}.
Proof. decide equality; apply Z.eq_dec. Defined.

Lemma existsb_perm {A} (f : A -> bool) l l' : Permutation l l' -> existsb f l = existsb f l'.
Proof. induction 1; cbn; [reflexivity|congruence|destruct (f x), (f y); reflexivity|congruence]. Qed.

(* The terms sum to the same total in any order of the moved windows: over the distinct tiles of
   their rectangles, each total counts the tiles some moved window left or took (terms_sum). *)
Theorem fresh_terms_perm seen moved moved' : Permutation moved moved' ->
  pairwise (map fst (seen ++ moved)) = true -> pairwise (map snd (seen ++ moved)) = true ->
  fold_left Z.add (fresh_terms seen moved) 0 = fold_left Z.add (fresh_terms seen moved') 0.
Proof.
  intros P Po Pn. set (U := nodup zz_dec (flat_map (fun p => cells (fst p) ++ cells (snd p)) moved)).
  assert (N : NoDup U) by apply NoDup_nodup.
  assert (C : forall l, incl l moved -> forall p c, In p l -> in_pair p c = true -> In c U).
  { intros l I p c Hp Hc. apply nodup_In, in_flat_map. exists p. split; [apply I, Hp|]. apply in_or_app.
    unfold in_pair in Hc. apply orb_true_iff in Hc as [H|H]; [left|right]; apply holds_cells, H. }
  assert (Pm : forall f : rect * rect -> rect, Permutation (map f (seen ++ moved)) (map f (seen ++ moved')))
    by (intros f; apply Permutation_map, Permutation_app_head, P).
  rewrite (terms_sum U seen moved 0 N (C moved (incl_refl _)) Po Pn).
  rewrite (terms_sum U seen moved' 0 N (C moved' (fun x H => Permutation_in _ (Permutation_sym P) H)))
    by (first [rewrite <- (pairwise_perm _ _ (Pm fst)); exact Po|rewrite <- (pairwise_perm _ _ (Pm snd)); exact Pn]).
  f_equal. apply count_ext. intros c. rewrite (existsb_perm _ _ _ P). reflexivity.
Qed.

Lemma fresh_terms_app seen l1 l2 : fresh_terms seen (l1 ++ l2) = fresh_terms seen l1 ++ fresh_terms (seen ++ l1) l2.
Proof.
  revert seen; induction l1 as [|[o n] l1 IH]; intros seen; cbn [app fresh_terms]; [rewrite app_nil_r; reflexivity|].
  rewrite IH, <- app_assoc. reflexivity.
Qed.

(* One step in any position: a window added anywhere in the moved list adds its term against
   the others (the search pushes windows in DFS order, Search::tally_with). *)
Theorem fresh_terms_insert seen l1 l2 o n :
  pairwise (map fst (seen ++ l1 ++ (o, n) :: l2)) = true -> pairwise (map snd (seen ++ l1 ++ (o, n) :: l2)) = true ->
  fold_left Z.add (fresh_terms seen (l1 ++ (o, n) :: l2)) 0 =
  fold_left Z.add (fresh_terms seen (l1 ++ l2)) 0 + fresh_term o n (seen ++ l1 ++ l2).
Proof.
  intros Po Pn. rewrite (fresh_terms_perm seen _ ((l1 ++ l2) ++ [(o, n)])) by
    (exact Po || exact Pn || (eapply perm_trans; [apply Permutation_sym, Permutation_middle|apply Permutation_cons_append])).
  rewrite fresh_terms_app, fold_left_app. reflexivity.
Qed.

(* So the search's tiles, its moved windows in the order it placed them, are rule 1's. *)
Corollary tiles_any_order (ws : list window) (cur : list rect) moved :
  NoDup (ids ws) -> Push.spans_ok ws -> length cur = length ws -> Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur ->
  pairwise (map wrect ws) = true -> pairwise cur = true -> Permutation moved (moved_pairs ws cur) ->
  fold_left Z.add (fresh_terms [] moved) 0 = Z.of_nat (Rules.displaced ws (relayout ws cur)).
Proof.
  intros N S L F Pw Pc P. rewrite <- (tiles_displaced ws cur N S L F Pw Pc).
  assert (Lw : length (map wrect ws) = length cur) by (rewrite length_map; lia).
  destruct (combine_fst_snd _ _ Lw) as [F1 F2]. symmetry. apply fresh_terms_perm; [apply Permutation_sym, P|..];
    cbn [app]; unfold moved_pairs; apply pairwise_filter; [rewrite F1|rewrite F2]; assumption.
Qed.

(* ---------- 3. The room ---------- *)

Lemma shared_overlaps a b : shared_area a b <> 0 -> overlaps a b = true.
Proof.
  unfold shared_area, overlaps, rows_overlap, cols_overlap, right, bottom; cbv zeta.
  destruct ((0 <? _) && (0 <? _)) eqn:P; [|congruence]. intros _.
  rewrite andb_true_iff, !Z.ltb_lt in P. rewrite !andb_true_iff, !Z.ltb_lt. lia.
Qed.

(* A candidate shares no tile with a rectangle that does not overlap the room. *)
Theorem room_local o a b r : on_grid o a b -> overlaps r (step_room o) = false -> shared_area (cand o a b) r = 0.
Proof.
  intros Hg R. destruct (cand_in o a b Hg) as [Hi Hj].
  destruct (Z.eq_dec (shared_area (cand o a b) r) 0) as [E|E]; [exact E|].
  apply shared_overlaps, (in_room o _ _ _ _ _ Hi Hj) in E. unfold step_room in R. congruence.
Qed.

(* Against every moved window, what a candidate's term subtracts is the Rust's held, over every
   moved window, plus freed, over those whose rectangle before overlaps the room. *)
Theorem counted_room o a b (moved : list (rect * rect)) : on_grid o a b ->
  counted o (cand o a b) moved =
  sumf (fun p => shared_area o (snd p)) moved +
  sumf (shared_area (cand o a b)) (filter (fun r => overlaps r (step_room o)) (map fst moved)).
Proof.
  intros Hg. rewrite counted_sum. induction moved as [|p l IH]; cbn [sumf map filter]; [reflexivity|].
  destruct (overlaps (fst p) (step_room o)) eqn:R; cbn [sumf]; [lia|]. rewrite (room_local o a b _ Hg R). lia.
Qed.

(* Hence the tiles of k leaving o for a candidate, as Search::step computes them. *)
Corollary fresh_room o a b (moved : list (rect * rect)) : on_grid o a b ->
  fresh_term o (cand o a b) moved =
  shared_area o o + shared_area (cand o a b) (cand o a b) - 2 * shared_area o (cand o a b) -
  (sumf (fun p => shared_area o (snd p)) moved +
   sumf (shared_area (cand o a b)) (filter (fun r => overlaps r (step_room o)) (map fst moved))).
Proof. intros Hg. unfold fresh_term. rewrite (counted_room o a b moved Hg). reflexivity. Qed.
