(* src/index.rs: the range tree over top-left corners, the queries the placement search and the
   island checks ask it (overlapping, touching), disjoint through it, shared_area, and the room
   every candidate of a search step lies in. Each query returns exactly what the scan it replaces
   returned, in the same ascending order. *)
From Stdlib Require Import ZArith List Bool Lia Permutation Sorted Mergesort Orders.
Import ListNotations.
From NewmCanvas Require Import Canvas.
Open Scope Z_scope.

(* The orders the Rust sorts by: (row, position) for a node's corners (sort_unstable on (i32,
   usize)), (column, position) for the corners before building (sort_unstable_by_key). *)
Module YK <: TotalLeBool.
  Definition t := (Z * nat)%type.
  Definition leb (a b : t) := (fst a <? fst b) || ((fst a =? fst b) && Nat.leb (snd a) (snd b)).
  Theorem leb_total : forall a b, leb a b = true \/ leb b a = true.
  Proof.
    intros [x i] [y j]; unfold leb; simpl.
    destruct (Z.lt_trichotomy x y) as [H|[H|H]].
    - left; apply Z.ltb_lt in H; rewrite H; reflexivity.
    - subst; rewrite Z.ltb_irrefl, Z.eqb_refl; simpl.
      destruct (Nat.le_ge_cases i j) as [L|L]; [left|right]; apply Nat.leb_le in L; rewrite L; reflexivity.
    - right; apply Z.ltb_lt in H; rewrite H; reflexivity.
  Qed.
End YK.
Module YSort := Sort YK.

Module XK <: TotalLeBool.
  Definition t := (Z * Z * nat)%type.
  Definition leb (a b : t) := let '(x, _, i) := a in let '(y, _, j) := b in
    (x <? y) || ((x =? y) && Nat.leb i j).
  Theorem leb_total : forall a b, leb a b = true \/ leb b a = true.
  Proof.
    intros [[x u] i] [[y v] j]; unfold leb.
    destruct (Z.lt_trichotomy x y) as [H|[H|H]].
    - left; apply Z.ltb_lt in H; rewrite H; reflexivity.
    - subst; rewrite Z.ltb_irrefl, Z.eqb_refl; simpl.
      destruct (Nat.le_ge_cases i j) as [L|L]; [left|right]; apply Nat.leb_le in L; rewrite L; reflexivity.
    - right; apply Z.ltb_lt in H; rewrite H; reflexivity.
  Qed.
End XK.
Module XSort := Sort XK.

(* Node: a leaf holds one corner; a bin knows the least and greatest column under it and keeps its
   corners as (row, position), sorted. *)
Inductive tree := Leaf (x y : Z) (k : nat) | Bin (xmin xmax : Z) (ys : list (Z * nat)) (l r : tree).

Definition ys_of t := match t with Leaf _ y k => [(y, k)] | Bin _ _ ys _ _ => ys end.
Definition xmin_of t := match t with Leaf x _ _ => x | Bin a _ _ _ _ => a end.
Definition xmax_of t := match t with Leaf x _ _ => x | Bin _ b _ _ _ => b end.

(* Node::build: one corner is a leaf; more split at half their number. fuel is the number of
   corners (the depth never exceeds it). *)
Fixpoint build (fuel : nat) (pts : list (Z * Z * nat)) : tree :=
  match pts with
  | [(x, y, k)] => Leaf x y k
  | _ => match fuel with
         | O => Leaf 0 0 0
         | S f => let h := (length pts / 2)%nat in
             let l := build f (firstn h pts) in let r := build f (skipn h pts) in
             Bin (Z.min (xmin_of l) (xmin_of r)) (Z.max (xmax_of l) (xmax_of r))
               (YSort.sort (ys_of l ++ ys_of r)) l r
         end
  end.

Fixpoint dropWhile {A} (p : A -> bool) (l : list A) :=
  match l with [] => [] | a :: l' => if p a then dropWhile p l' else l end.
Fixpoint takeWhile {A} (p : A -> bool) (l : list A) :=
  match l with [] => [] | a :: l' => if p a then a :: takeWhile p l' else [] end.

(* Node::corners: the positions whose corner lies in columns x1..=x2 and rows y1..=y2. A whole
   bin is read by partition_point (the first row >= y1: dropWhile on its sorted rows) and
   take_while (row <= y2). *)
Fixpoint corners (t : tree) (x1 x2 y1 y2 : Z) : list nat :=
  match t with
  | Leaf x y k => if (x1 <=? x) && (x <=? x2) && (y1 <=? y) && (y <=? y2) then [k] else []
  | Bin xmin xmax ys l r =>
      if (xmax <? x1) || (x2 <? xmin) then []
      else if (x1 <=? xmin) && (xmax <=? x2) then
        map snd (takeWhile (fun p => fst p <=? y2) (dropWhile (fun p => fst p <? y1) ys))
      else corners l x1 x2 y1 y2 ++ corners r x1 x2 y1 y2
  end.

Record index := Index { rects : list rect; root : option tree; wide : Z; tall : Z }.

Definition d0 := Rect 0 0 0 0.
Definition corner_pts (rs : list rect) :=
  map (fun k => (ri (nth k rs d0), rj (nth k rs d0), k)) (seq 0 (length rs)).

(* Index::new: corners sorted by column then position, the tree over them when there are any,
   and the widest and tallest rectangle (fold from 0). *)
Definition index_new (rs : list rect) :=
  let pts := XSort.sort (corner_pts rs) in
  Index rs (match pts with [] => None | _ => Some (build (length pts) pts) end)
    (fold_left Z.max (map rw rs) 0) (fold_left Z.max (map rh rs) 0).

(* Index::overlapping: the corners in q widened left by wide and up by tall, those whose
   rectangle overlaps q, sorted. *)
Definition overlapping (ix : index) (q : rect) : list nat :=
  NatSort.sort (filter (fun k => overlaps (nth k (rects ix) d0) q)
    (match root ix with
     | None => []
     | Some t => corners t (ri q - wide ix + 1) (right q - 1) (rj q - tall ix + 1) (bottom q - 1)
     end)).

(* ring and Index::touching_into: the rectangles overlapping the ring of tiles around a that
   share a side with it. *)
Definition ring a := Rect (ri a - 1) (rj a - 1) (rw a + 2) (rh a + 2).
Definition touching (ix : index) (a : rect) :=
  filter (fun b => touch a (nth b (rects ix) d0)) (overlapping ix (ring a)).

(* place::disjoint: each rectangle overlaps none but itself. *)
Definition disjoint_fast (rs : list rect) :=
  let ix := index_new rs in
  forallb (fun k => forallb (fun o => Nat.eqb o k) (overlapping ix (nth k rs d0))) (seq 0 (length rs)).

(* index::shared_area: the tiles two rectangles share. *)
Definition shared_area a b :=
  let w := Z.min (right a) (right b) - Z.max (ri a) (ri b) in
  let h := Z.min (bottom a) (bottom b) - Z.max (rj a) (rj b) in
  if (0 <? w) && (0 <? h) then w * h else 0.

(* The search step's room: every candidate for o lies in it. *)
Definition room o rx ry := Rect (ri o - rx) (rj o - ry) (rw o + 2 * rx) (rh o + 2 * ry).
Definition candidate o i j := Rect i j (rw o) (rh o).

(* ---------- Lists ---------- *)

Lemma natleb_le x y : NatOrder.leb x y = true <-> (x <= y)%nat.
Proof.
  revert y; induction x as [|x IH]; intros [|y]; simpl; split; intros H; try lia; try reflexivity; try discriminate.
  - apply IH in H; lia.
  - apply IH; lia.
Qed.

Lemma sorted_perm_eq (l1 l2 : list nat) :
  Sorted (fun a b => is_true (NatOrder.leb a b)) l1 -> Sorted (fun a b => is_true (NatOrder.leb a b)) l2 ->
  Permutation l1 l2 -> l1 = l2.
Proof.
  assert (T : Transitive (fun a b => is_true (NatOrder.leb a b))).
  { intros x y z H1 H2; unfold is_true in *; apply natleb_le in H1, H2; apply natleb_le; lia. }
  intros S1 S2; apply (Sorted_StronglySorted T) in S1, S2. revert l2 S2.
  induction S1 as [|a t S1 IH F1]; intros l2 S2 P.
  - apply Permutation_nil in P; subst; reflexivity.
  - destruct S2 as [|b t' S2 F2].
    + apply Permutation_sym, Permutation_nil in P; discriminate.
    + assert (a = b) as <-.
      { assert (Ia : In a (b :: t')) by (eapply Permutation_in; [exact P|left; reflexivity]).
        assert (Ib : In b (a :: t)) by (eapply Permutation_in; [symmetry; exact P|left; reflexivity]).
        destruct Ia as [->|Ia]; [reflexivity|]. destruct Ib as [->|Ib]; [reflexivity|].
        rewrite Forall_forall in F1, F2. specialize (F1 b Ib); specialize (F2 a Ia).
        unfold is_true in *; apply natleb_le in F1, F2; lia. }
      f_equal; apply IH; [exact S2|]. exact (Permutation_cons_inv P).
Qed.

Lemma filter_seq_sorted f a n : Sorted (fun x y => is_true (NatOrder.leb x y)) (filter f (seq a n)).
Proof.
  apply StronglySorted_Sorted. revert a; induction n as [|n IH]; intros a; simpl; [constructor|].
  destruct (f a); [|apply IH]. constructor; [apply IH|].
  apply Forall_forall; intros x Hx; apply filter_In in Hx as [Hx _]; apply in_seq in Hx.
  unfold is_true; apply natleb_le; lia.
Qed.

Lemma sort_filter_seq f n (l : list nat) : Permutation l (filter f (seq 0 n)) -> NatSort.sort l = filter f (seq 0 n).
Proof.
  intros P; apply sorted_perm_eq; [apply NatSort.Sorted_sort|apply filter_seq_sorted|].
  rewrite <- P; symmetry; apply NatSort.Permuted_sort.
Qed.

Lemma perm_filter {A} (f : A -> bool) l l' : Permutation l l' -> Permutation (filter f l) (filter f l').
Proof.
  induction 1 as [|x l l' P IH|x y l|l l' l'' P1 IH1 P2 IH2]; simpl; [constructor| | |eapply Permutation_trans; eassumption].
  - destruct (f x); [constructor|]; exact IH.
  - destruct (f x), (f y); [apply perm_swap|apply Permutation_refl|apply Permutation_refl|apply Permutation_refl].
Qed.

Lemma perm_of_eq {A} (l l' : list A) : l = l' -> Permutation l l'.
Proof. intros ->; reflexivity. Qed.

Lemma filter_filter_and {A} (f g : A -> bool) l : filter f (filter g l) = filter (fun x => g x && f x) l.
Proof.
  induction l as [|x l IH]; simpl; [reflexivity|].
  destruct (g x); simpl; [destruct (f x); simpl|]; rewrite ?IH; reflexivity.
Qed.

Lemma filter_map_comm {A B} (f : B -> bool) (g : A -> B) l : filter f (map g l) = map g (filter (fun x => f (g x)) l).
Proof. induction l as [|x l IH]; simpl; [reflexivity|]. destruct (f (g x)); simpl; rewrite IH; reflexivity. Qed.

Lemma filter_none {A} (f : A -> bool) l : (forall x, In x l -> f x = false) -> filter f l = [].
Proof. induction l as [|x l IH]; intros H; simpl; [reflexivity|]. rewrite H by (left; reflexivity). apply IH; intros; apply H; right; assumption. Qed.

Lemma filter_all {A} (f : A -> bool) l : (forall x, In x l -> f x = true) -> filter f l = l.
Proof. induction l as [|x l IH]; intros H; simpl; [reflexivity|]. rewrite H by (left; reflexivity). f_equal; apply IH; intros; apply H; right; assumption. Qed.

(* ---------- Rows of a sorted node ---------- *)

Lemma yk_leb_fst a b : YK.leb a b = true -> fst a <= fst b.
Proof. unfold YK.leb; intros H; apply orb_true_iff in H as [H|H]; [apply Z.ltb_lt in H; lia|]. apply andb_true_iff in H as [H _]; apply Z.eqb_eq in H; lia. Qed.

Lemma yk_trans : Transitive (fun a b => is_true (YK.leb a b)).
Proof.
  intros [x i] [y j] [z k]; unfold is_true, YK.leb; simpl.
  rewrite !orb_true_iff, !andb_true_iff, !Z.ltb_lt, !Z.eqb_eq, !Nat.leb_le; lia.
Qed.

Lemma take_sorted y2 (l : list (Z * nat)) : StronglySorted (fun a b => is_true (YK.leb a b)) l ->
  takeWhile (fun p => fst p <=? y2) l = filter (fun p => fst p <=? y2) l.
Proof.
  induction 1 as [|a l S IH F]; simpl; [reflexivity|].
  destruct (fst a <=? y2) eqn:A; [rewrite IH; reflexivity|].
  symmetry; apply filter_none; intros b Hb. rewrite Forall_forall in F; apply F, yk_leb_fst in Hb.
  apply Z.leb_gt in A; apply Z.leb_gt; lia.
Qed.

Lemma drop_sorted y1 (l : list (Z * nat)) : StronglySorted (fun a b => is_true (YK.leb a b)) l ->
  dropWhile (fun p => fst p <? y1) l = filter (fun p => y1 <=? fst p) l /\
  StronglySorted (fun a b => is_true (YK.leb a b)) (dropWhile (fun p => fst p <? y1) l).
Proof.
  induction 1 as [|a l S IH F]; simpl; [split; constructor|].
  destruct (fst a <? y1) eqn:A.
  - replace (y1 <=? fst a) with false by (symmetry; apply Z.leb_gt; apply Z.ltb_lt in A; lia). exact IH.
  - replace (y1 <=? fst a) with true by (symmetry; apply Z.leb_le; apply Z.ltb_ge in A; lia).
    split; [|constructor; assumption]. f_equal; symmetry; apply filter_all; intros b Hb.
    rewrite Forall_forall in F; apply F, yk_leb_fst in Hb. apply Z.ltb_ge in A; apply Z.leb_le; lia.
Qed.

Lemma rows_sorted y1 y2 (l : list (Z * nat)) : Sorted (fun a b => is_true (YK.leb a b)) l ->
  takeWhile (fun p => fst p <=? y2) (dropWhile (fun p => fst p <? y1) l) =
  filter (fun p => (y1 <=? fst p) && (fst p <=? y2)) l.
Proof.
  intros S; apply (Sorted_StronglySorted yk_trans) in S. destruct (drop_sorted y1 l S) as [D DS].
  rewrite take_sorted by exact DS. rewrite D, filter_filter_and. reflexivity.
Qed.

(* ---------- The tree ---------- *)

Definition key (p : Z * Z * nat) := let '(_, _, k) := p in k.
Definition in_box x1 x2 y1 y2 (p : Z * Z * nat) :=
  let '(x, y, _) := p in (x1 <=? x) && (x <=? x2) && (y1 <=? y) && (y <=? y2).
Definition yk (p : Z * Z * nat) := let '(_, y, k) := p in (y, k).
Definition col (p : Z * Z * nat) := let '(x, _, _) := p in x.

Lemma build_ok fuel pts : pts <> [] -> (length pts <= fuel)%nat ->
  (forall x1 x2 y1 y2, Permutation (corners (build fuel pts) x1 x2 y1 y2) (map key (filter (in_box x1 x2 y1 y2) pts))) /\
  (forall p, In p pts -> xmin_of (build fuel pts) <= col p <= xmax_of (build fuel pts)) /\
  Permutation (ys_of (build fuel pts)) (map yk pts).
Proof.
  revert pts; induction fuel as [|f IH]; intros pts NE Len.
  { destruct pts; [contradiction|simpl in Len; lia]. }
  destruct pts as [|p [|p' rest]]; [contradiction| |].
  { destruct p as [[x y] k]; simpl. split; [|split].
    - intros x1 x2 y1 y2; destruct ((x1 <=? x) && (x <=? x2) && (y1 <=? y) && (y <=? y2)); simpl; constructor; constructor.
    - intros q [<-|[]]; simpl; lia.
    - constructor; constructor. }
  set (pts := p :: p' :: rest) in *.
  assert (B : build (S f) pts = let h := (length pts / 2)%nat in
             let l := build f (firstn h pts) in let r := build f (skipn h pts) in
             Bin (Z.min (xmin_of l) (xmin_of r)) (Z.max (xmax_of l) (xmax_of r))
               (YSort.sort (ys_of l ++ ys_of r)) l r) by (subst pts; destruct p as [[? ?] ?]; reflexivity).
  rewrite B; clear B; cbv zeta.
  set (h := (length pts / 2)%nat). set (L := firstn h pts). set (R := skipn h pts).
  assert (HL : length pts = S (S (length rest))) by reflexivity.
  assert (H1 : (1 <= h)%nat) by (unfold h; rewrite HL; apply Nat.div_le_lower_bound; lia).
  assert (H2 : (h < length pts)%nat) by (unfold h; apply Nat.div_lt; [rewrite HL|]; lia).
  assert (LL : length L = h) by (apply firstn_length_le; lia).
  assert (LR : length R = (length pts - h)%nat) by apply length_skipn.
  rewrite HL in Len, H2, LR.
  assert (NL : L <> []) by (intros E; apply (f_equal (@length _)) in E; simpl in E; lia).
  assert (NR : R <> []) by (intros E; apply (f_equal (@length _)) in E; simpl in E; lia).
  destruct (IH L NL ltac:(lia)) as (CL & BL & YL). destruct (IH R NR ltac:(lia)) as (CR & BR & YR).
  assert (E : pts = L ++ R) by (symmetry; apply firstn_skipn).
  assert (Bd : forall q, In q pts -> Z.min (xmin_of (build f L)) (xmin_of (build f R)) <= col q <=
                                     Z.max (xmax_of (build f L)) (xmax_of (build f R))).
  { intros q Hq; rewrite E in Hq; apply in_app_or in Hq as [Hq|Hq];
      [specialize (BL q Hq)|specialize (BR q Hq)]; lia. }
  split; [|split; [exact Bd|]].
  2:{ cbn [ys_of]. eapply Permutation_trans; [symmetry; apply YSort.Permuted_sort|].
      rewrite E, map_app. apply Permutation_app; assumption. }
  intros x1 x2 y1 y2; cbn [corners].
  destruct ((Z.max (xmax_of (build f L)) (xmax_of (build f R)) <? x1) ||
            (x2 <? Z.min (xmin_of (build f L)) (xmin_of (build f R)))) eqn:Out.
  { rewrite filter_none; [constructor|]. intros [[x y] k] Hq; specialize (Bd _ Hq); simpl in Bd |- *.
    apply orb_true_iff in Out as [O|O]; apply Z.ltb_lt in O;
      rewrite !andb_false_iff; rewrite ?Z.leb_gt; lia. }
  destruct ((x1 <=? Z.min (xmin_of (build f L)) (xmin_of (build f R))) &&
            (Z.max (xmax_of (build f L)) (xmax_of (build f R)) <=? x2)) eqn:In_.
  - rewrite rows_sorted by apply YSort.Sorted_sort.
    rewrite (perm_filter _ _ _ (Permutation_trans (Permutation_sym (YSort.Permuted_sort _))
      (Permutation_trans (Permutation_app YL YR) (Permutation_refl _)))).
    rewrite <- map_app, <- E, filter_map_comm, map_map.
    replace (filter (in_box x1 x2 y1 y2) pts) with (filter (fun x => (y1 <=? fst (yk x)) && (fst (yk x) <=? y2)) pts).
    + apply perm_of_eq; apply map_ext; intros [[? ?] ?]; reflexivity.
    + apply filter_ext_in; intros [[x y] k] Hq; specialize (Bd _ Hq); simpl in Bd |- *.
      apply andb_true_iff in In_ as [A1 A2]; apply Z.leb_le in A1, A2.
      replace (x1 <=? x) with true by (symmetry; apply Z.leb_le; lia).
      replace (x <=? x2) with true by (symmetry; apply Z.leb_le; lia). reflexivity.
  - rewrite E, filter_app, map_app. apply Permutation_app; [apply CL|apply CR].
Qed.

Lemma fold_max_ge (l : list Z) acc x : In x l -> x <= fold_left Z.max l acc.
Proof.
  assert (M : forall l acc, acc <= fold_left Z.max l acc).
  { intros l0; induction l0 as [|y l0 IH]; intros a; simpl; [lia|]. specialize (IH (Z.max a y)); lia. }
  revert acc; induction l as [|y l IH]; intros acc H; [destruct H|]. simpl.
  destruct H as [->|Hx]; [specialize (M l (Z.max acc x)); lia|apply IH, Hx].
Qed.

Lemma nth_map_seq {A} (f : nat -> A) n k d : (k < n)%nat -> nth k (map f (seq 0 n)) d = f k.
Proof.
  intros H; rewrite nth_indep with (d' := f O) by (rewrite length_map, length_seq; exact H).
  rewrite map_nth, seq_nth by exact H; reflexivity.
Qed.

Lemma nth_corner rs k : (k < length rs)%nat -> nth k (corner_pts rs) (0, 0, O) = (ri (nth k rs d0), rj (nth k rs d0), k).
Proof.
  intros H; unfold corner_pts; rewrite nth_map_seq by exact H; reflexivity.
Qed.

(* A rectangle that overlaps q has its corner in q widened by the widest and tallest held. *)
Lemma widened rs q k : (k < length rs)%nat -> overlaps (nth k rs d0) q = true ->
  in_box (ri q - fold_left Z.max (map rw rs) 0 + 1) (right q - 1)
    (rj q - fold_left Z.max (map rh rs) 0 + 1) (bottom q - 1) (nth k (corner_pts rs) (0, 0, O)) = true.
Proof.
  intros H O; rewrite nth_corner by exact H.
  assert (W := fold_max_ge (map rw rs) 0 (rw (nth k rs d0)) (in_map _ _ _ (nth_In _ _ H))).
  assert (T := fold_max_ge (map rh rs) 0 (rh (nth k rs d0)) (in_map _ _ _ (nth_In _ _ H))).
  revert O W T; generalize (nth k rs d0); intros r O W T.
  unfold overlaps, rows_overlap, cols_overlap, right, bottom in *; simpl.
  rewrite !andb_true_iff, !Z.ltb_lt in O; rewrite !andb_true_iff, !Z.leb_le; lia.
Qed.

Lemma corner_pts_seq rs : corner_pts rs = map (fun k => (ri (nth k rs d0), rj (nth k rs d0), k)) (seq 0 (length rs)).
Proof. reflexivity. Qed.

(* The range tree answers exactly what the scan of every rectangle answered, in its order. *)
Theorem overlapping_scan rs q :
  overlapping (index_new rs) q = filter (fun k => overlaps (nth k rs d0) q) (seq 0 (length rs)).
Proof.
  unfold overlapping, index_new; cbv zeta; cbn [root rects wide tall]. apply sort_filter_seq.
  set (W := fold_left Z.max (map rw rs) 0). set (T := fold_left Z.max (map rh rs) 0).
  set (bx := in_box (ri q - W + 1) (right q - 1) (rj q - T + 1) (bottom q - 1)).
  assert (Goal : Permutation (filter (fun k => overlaps (nth k rs d0) q) (map key (filter bx (corner_pts rs))))
                             (filter (fun k => overlaps (nth k rs d0) q) (seq 0 (length rs)))).
  { rewrite corner_pts_seq, (filter_map_comm bx), map_map.
    erewrite (map_ext _ (fun x => x)) by (intros; reflexivity). rewrite map_id.
    apply perm_of_eq. rewrite filter_filter_and. apply filter_ext_in; intros k Hk.
    apply in_seq in Hk. destruct (overlaps (nth k rs d0) q) eqn:O; [|apply andb_false_r].
    rewrite andb_true_r. pose proof (widened rs q k ltac:(lia) O) as Wd. rewrite nth_corner in Wd by lia. exact Wd. }
  destruct (XSort.sort (corner_pts rs)) as [|p ps] eqn:Srt.
  - pose proof (XSort.Permuted_sort (corner_pts rs)) as P; rewrite Srt in P.
    apply Permutation_sym, Permutation_nil in P. rewrite <- Goal, P; constructor.
  - rewrite <- Goal. apply perm_filter.
    destruct (build_ok (length (p :: ps)) (p :: ps) ltac:(discriminate) (le_n _)) as [C _].
    unfold bx; rewrite C. apply Permutation_map, perm_filter.
    rewrite <- Srt; symmetry; apply XSort.Permuted_sort.
Qed.

Lemma touch_ring a b : 0 <= rw a -> 0 <= rh a -> 0 <= rw b -> 0 <= rh b -> touch a b = true -> overlaps b (ring a) = true.
Proof.
  unfold touch, overlaps, rows_overlap, cols_overlap, ring, right, bottom; simpl; intros.
  repeat rewrite ?orb_true_iff, ?andb_true_iff, ?Z.eqb_eq, ?Z.ltb_lt in *. lia.
Qed.

Theorem touching_scan rs a : Forall (fun r => 0 <= rw r /\ 0 <= rh r) rs -> 0 <= rw a -> 0 <= rh a ->
  touching (index_new rs) a = filter (fun b => touch a (nth b rs d0)) (seq 0 (length rs)).
Proof.
  intros F Wa Ha; unfold touching; simpl; rewrite overlapping_scan, filter_filter_and.
  apply filter_ext_in; intros b Hb; apply in_seq in Hb.
  destruct (touch a (nth b rs d0)) eqn:T; [|apply andb_false_r]. rewrite andb_true_r.
  rewrite Forall_forall in F; destruct (F _ (nth_In rs d0 (proj2 Hb))). apply touch_ring; assumption.
Qed.

Lemma overlaps_sym a b : overlaps a b = overlaps b a.
Proof. unfold overlaps, rows_overlap, cols_overlap; destruct (rj a <? bottom b), (rj b <? bottom a), (ri a <? right b), (ri b <? right a); reflexivity. Qed.

Lemma pairwise_nth rs : pairwise rs = true <->
  forall i j, (i < j < length rs)%nat -> overlaps (nth i rs d0) (nth j rs d0) = false.
Proof.
  induction rs as [|r rs IH]; simpl.
  { split; [intros _ i j H; simpl in H; lia|intros _; reflexivity]. }
  rewrite andb_true_iff, forallb_forall, IH; split.
  - intros [A B] [|i] [|j] H; simpl; try lia.
    + apply negb_true_iff, A, nth_In; lia.
    + apply B; lia.
  - intros H; split.
    + intros q Hq; apply In_nth with (d := d0) in Hq as (j & Hj & <-). apply negb_true_iff, (H O (S j)); simpl; lia.
    + intros i j Hij; apply (H (S i) (S j)); simpl; lia.
Qed.

Theorem disjoint_fast_pairwise rs : disjoint_fast rs = pairwise rs.
Proof.
  apply eq_iff_eq_true; unfold disjoint_fast; rewrite pairwise_nth, forallb_forall; split.
  - intros H i j Hij. destruct (overlaps (nth i rs d0) (nth j rs d0)) eqn:O; [|reflexivity].
    specialize (H j ltac:(apply in_seq; lia)). rewrite overlapping_scan, forallb_forall in H.
    specialize (H i ltac:(apply filter_In; split; [apply in_seq; lia|exact O])). apply Nat.eqb_eq in H; lia.
  - intros H k Hk; apply in_seq in Hk; rewrite overlapping_scan, forallb_forall; intros o Ho.
    apply filter_In in Ho as [Ho O]; apply in_seq in Ho; apply Nat.eqb_eq.
    destruct (Nat.lt_trichotomy o k) as [L|[E|L]]; [|exact E|].
    + rewrite H in O by lia; discriminate.
    + rewrite overlaps_sym, H in O by lia; discriminate.
Qed.

(* shared_area counts the tiles of a inside b. *)
Definition cells r := flat_map (fun x => map (fun y => (x, y)) (range (rj r) (bottom r))) (range (ri r) (right r)).
Definition holds r (c : Z * Z) := (ri r <=? fst c) && (fst c <? right r) && (rj r <=? snd c) && (snd c <? bottom r).

Lemma range_count a b lo hi :
  Z.of_nat (length (filter (fun y => (lo <=? y) && (y <? hi)) (range a b))) = Z.max 0 (Z.min b hi - Z.max a lo).
Proof.
  unfold range. destruct (Z.le_gt_cases b a) as [Le|Gt].
  { replace (Z.to_nat (b - a)) with O by lia; simpl; lia. }
  replace b with (a + Z.of_nat (Z.to_nat (b - a))) at 2 by lia.
  induction (Z.to_nat (b - a)) as [|m IH]; [simpl; lia|].
  rewrite seq_S, map_app, filter_app, length_app, Nat2Z.inj_add; simpl. rewrite IH.
  destruct ((lo <=? a + Z.of_nat m) && (a + Z.of_nat m <? hi)) eqn:C; simpl;
    rewrite ?andb_true_iff, ?andb_false_iff, ?Z.leb_le, ?Z.leb_gt, ?Z.ltb_lt, ?Z.ltb_ge in C; lia.
Qed.

Lemma sum_const {A} (p : A -> bool) (n : nat) l :
  fold_right Nat.add O (map (fun x => if p x then n else O) l) = (length (filter p l) * n)%nat.
Proof. induction l as [|x l IH]; simpl; [reflexivity|]. destruct (p x); simpl; lia. Qed.

Lemma length_filter_flat_map {A B} (f : B -> bool) (g : A -> list B) l :
  length (filter f (flat_map g l)) = fold_right Nat.add O (map (fun x => length (filter f (g x))) l).
Proof. induction l as [|x l IH]; simpl; [reflexivity|]. rewrite filter_app, length_app, IH; reflexivity. Qed.

Theorem shared_area_cells a b : Z.of_nat (length (filter (holds b) (cells a))) = shared_area a b.
Proof.
  unfold cells; rewrite length_filter_flat_map.
  set (hn := length (filter (fun y => (rj b <=? y) && (y <? bottom b)) (range (rj a) (bottom a)))).
  rewrite (map_ext_in _ (fun x => if (ri b <=? x) && (x <? right b) then hn else O)).
  2:{ intros x _. rewrite filter_map_comm, length_map. unfold holds; cbn [fst snd].
      destruct ((ri b <=? x) && (x <? right b)) eqn:X.
      - unfold hn; f_equal; apply filter_ext; intros y; rewrite X; reflexivity.
      - rewrite filter_none; [reflexivity|]. intros y _; reflexivity. }
  rewrite sum_const, Nat2Z.inj_mul, range_count. unfold hn; rewrite range_count.
  unfold shared_area. destruct ((0 <? Z.min (right a) (right b) - Z.max (ri a) (ri b)) &&
                                (0 <? Z.min (bottom a) (bottom b) - Z.max (rj a) (rj b))) eqn:P;
    rewrite ?andb_true_iff, ?andb_false_iff, ?Z.ltb_lt, ?Z.ltb_ge in P; nia.
Qed.
