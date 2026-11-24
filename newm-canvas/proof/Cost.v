(* src/place.rs Search::total's tile term from shared areas. With the windows before disjoint and
   the moved ones after disjoint, a tile a moved window left or took changes owner and one it kept
   does not, so each moved window, in window order, adds the tiles of its old and new rectangles
   but not both that no window moved before it holds (fresh_term_cells); over a layout with
   nothing overlapping, they sum to rule 1's tiles reassigned (Rules.displaced). *)
From Stdlib Require Import ZArith List Bool Lia Permutation.
Import ListNotations.
From NewmCanvas Require Import Canvas Push Rules Index.
Open Scope Z_scope.

(* Search::total: for each moved window (o, n), its old and new areas less twice what they
   share, less what each shares with the other's counterpart in a window moved before it. *)
Definition counted (o n : rect) (seen : list (rect * rect)) :=
  fold_left (fun acc p => acc + (shared_area o (snd p) + shared_area n (fst p))) seen 0.
Definition fresh_term (o n : rect) (seen : list (rect * rect)) :=
  shared_area o o + shared_area n n - 2 * shared_area o n - counted o n seen.
Fixpoint fresh_terms (seen : list (rect * rect)) (moved : list (rect * rect)) : list Z :=
  match moved with
  | [] => []
  | (o, n) :: rest => fresh_term o n seen :: fresh_terms (seen ++ [(o, n)]) rest
  end.
Definition moved_pairs (ws : list window) (cur : list rect) :=
  filter (fun p => negb (rect_eqb (fst p) (snd p))) (combine (map wrect ws) cur).

Definition in_pair (p : rect * rect) c := holds (fst p) c || holds (snd p) c.
Definition count (U : list (Z * Z)) (f : Z * Z -> bool) := Z.of_nat (length (filter f U)).

(* ---------- Tiles of a rectangle, and counting them ---------- *)

Definition grid x0 x1 y0 y1 := flat_map (fun x => map (fun y => (x, y)) (range y0 y1)) (range x0 x1).

Lemma in_range a b z : In z (range a b) <-> a <= z < b.
Proof.
  unfold range. rewrite in_map_iff. split.
  - intros [k [<- Hk]]. apply in_seq in Hk. lia.
  - intros H. exists (Z.to_nat (z - a)). split; [lia|]. apply in_seq. lia.
Qed.

Lemma nodup_range a b : NoDup (range a b).
Proof.
  unfold range. generalize 0%nat. induction (Z.to_nat (b - a)) as [|m IH]; intros s; cbn; constructor; auto.
  rewrite in_map_iff. intros [k [E Hk]]. apply in_seq in Hk. lia.
Qed.

Lemma in_grid x0 x1 y0 y1 c : In c (grid x0 x1 y0 y1) <-> x0 <= fst c < x1 /\ y0 <= snd c < y1.
Proof.
  destruct c as [x y]. unfold grid. rewrite in_flat_map. cbn [fst snd]. split.
  - intros [x' [Hx Hy]]. apply in_map_iff in Hy as [y' [E Hy]]. inversion E; subst.
    rewrite in_range in Hx, Hy. lia.
  - intros [Hx Hy]. exists x. split; [apply in_range; lia|]. apply in_map_iff. exists y. split; [reflexivity|apply in_range; lia].
Qed.

Lemma nodup_grid x0 x1 y0 y1 : NoDup (grid x0 x1 y0 y1).
Proof.
  unfold grid. generalize (nodup_range x0 x1). generalize (range x0 x1). intros xs N.
  induction N as [|x xs Nx Nxs IH]; cbn; [constructor|].
  apply NoDup_app; auto.
  - generalize (nodup_range y0 y1). generalize (range y0 y1). intros ys M.
    induction M as [|y ys Ny M IHM]; cbn; constructor; auto.
    rewrite in_map_iff. intros [y' [E Hy]]. inversion E; subst. contradiction.
  - intros c H1 H2. apply in_map_iff in H1 as [y [<- _]]. apply in_flat_map in H2 as [x' [Hx' H2]].
    apply in_map_iff in H2 as [y' [E _]]. inversion E; subst. contradiction.
Qed.

Lemma holds_cells r c : In c (cells r) <-> holds r c = true.
Proof.
  change (cells r) with (grid (ri r) (right r) (rj r) (bottom r)). rewrite in_grid. unfold holds.
  rewrite !andb_true_iff, !Z.leb_le, !Z.ltb_lt. tauto.
Qed.

Lemma nodup_cells r : NoDup (cells r).
Proof. apply nodup_grid. Qed.

Lemma count_ext U f g : (forall c, f c = g c) -> count U f = count U g.
Proof. intros H. unfold count. rewrite (filter_ext f g H). reflexivity. Qed.

(* Over distinct tiles holding a's, the tiles of a inside b number shared_area a b. *)
Lemma count_shared U a b : NoDup U -> (forall c, holds a c = true -> In c U) ->
  count U (fun c => holds a c && holds b c) = shared_area a b.
Proof.
  intros N C. rewrite <- shared_area_cells. unfold count. f_equal. apply Permutation_length.
  apply NoDup_Permutation; [apply NoDup_filter; exact N|apply NoDup_filter, nodup_cells|].
  intros c. rewrite !filter_In, andb_true_iff, holds_cells.
  split; [intros [_ [H1 H2]]; split; assumption|intros [H1 H2]; split; [apply C, H1|split; assumption]].
Qed.

Definition ind (b : bool) := if b then 1 else 0.
Fixpoint sumf {A} (F : A -> Z) (l : list A) : Z := match l with [] => 0 | a :: l' => F a + sumf F l' end.

Lemma count_sum U f : count U f = sumf (fun c => ind (f c)) U.
Proof. unfold count. induction U as [|c U IH]; cbn; [reflexivity|]. destruct (f c); cbn [length ind]; lia. Qed.

Lemma sumf_ext {A} (F G : A -> Z) l : (forall x, In x l -> F x = G x) -> sumf F l = sumf G l.
Proof.
  induction l as [|a l IH]; cbn; intros H; [reflexivity|].
  rewrite (H a (or_introl eq_refl)), IH; [reflexivity|]. intros x Hx. apply H. right. exact Hx.
Qed.

Lemma sumf_add {A} (F G : A -> Z) l : sumf (fun x => F x + G x) l = sumf F l + sumf G l.
Proof. induction l; cbn [sumf]; lia. Qed.

Lemma sumf_lin {A} (F1 F2 F3 F4 : A -> Z) l :
  sumf (fun x => F1 x + F2 x - 2 * F3 x - F4 x) l = sumf F1 l + sumf F2 l - 2 * sumf F3 l - sumf F4 l.
Proof. induction l; cbn [sumf]; lia. Qed.

Lemma sumf_scale {A} k1 k2 (F G : A -> Z) l : sumf (fun x => k1 * F x + k2 * G x) l = k1 * sumf F l + k2 * sumf G l.
Proof. induction l as [|a l IH]; cbn [sumf]; [ring|]. rewrite IH. ring. Qed.

Lemma sumf_swap {A B} (H : A -> B -> Z) (l : list A) (U : list B) :
  sumf (fun p => sumf (H p) U) l = sumf (fun c => sumf (fun p => H p c) l) U.
Proof.
  induction l as [|a l IH]; cbn.
  - induction U; cbn; lia.
  - rewrite IH, <- sumf_add. reflexivity.
Qed.

Lemma sumf_map {A B} (F : B -> Z) (g : A -> B) l : sumf F (map g l) = sumf (fun x => F (g x)) l.
Proof. induction l; cbn; congruence. Qed.

Lemma count_or U f g : (forall c, f c && g c = false) -> count U f + count U g = count U (fun c => f c || g c).
Proof.
  intros H. rewrite !count_sum. induction U as [|c U IH]; cbn [sumf]; [reflexivity|].
  specialize (H c). destruct (f c), (g c); cbn [ind orb andb] in *; try discriminate; lia.
Qed.

Lemma counted_sum o n seen :
  counted o n seen = sumf (fun p => shared_area o (snd p) + shared_area n (fst p)) seen.
Proof.
  unfold counted.
  assert (G : forall a, fold_left (fun acc p => acc + (shared_area o (snd p) + shared_area n (fst p))) seen a =
    a + sumf (fun p => shared_area o (snd p) + shared_area n (fst p)) seen).
  { induction seen as [|p seen IH]; intros a; cbn; [lia|]. rewrite IH. lia. }
  rewrite G. lia.
Qed.

(* ---------- Disjoint rectangles share no tile ---------- *)

Lemma common_overlaps a b c : holds a c = true -> holds b c = true -> overlaps a b = true.
Proof.
  unfold holds, overlaps, rows_overlap, cols_overlap. rewrite !andb_true_iff, !Z.leb_le, !Z.ltb_lt.
  intros H1 H2. repeat split; lia.
Qed.

Lemma pw_head r rs c : pairwise (r :: rs) = true -> holds r c = true -> existsb (fun q => holds q c) rs = false.
Proof.
  cbn [pairwise]. rewrite andb_true_iff, forallb_forall. intros [H _] Hr. apply not_true_iff_false. intros E.
  apply existsb_exists in E as [q [Hq Hc]]. specialize (H q Hq). rewrite (common_overlaps _ _ _ Hr Hc) in H. cbn in H. discriminate.
Qed.

Lemma ind_exists (l : list rect) c : pairwise l = true ->
  sumf (fun r => ind (holds r c)) l = ind (existsb (fun r => holds r c) l).
Proof.
  induction l as [|r l IH]; cbn [sumf existsb]; [reflexivity|]. intros P. pose proof (pw_head r l c P) as H.
  cbn [pairwise] in P. apply andb_true_iff in P as [_ P]. rewrite IH by exact P.
  destruct (holds r c); cbn [ind orb]; [rewrite (H eq_refl)|]; reflexivity.
Qed.

Lemma forallb_perm {A} (f : A -> bool) l l' : Permutation l l' -> forallb f l = forallb f l'.
Proof. induction 1; cbn; [reflexivity|congruence|destruct (f x), (f y); reflexivity|congruence]. Qed.

Lemma pairwise_perm l l' : Permutation l l' -> pairwise l = pairwise l'.
Proof.
  intros P. induction P as [|x l l' P IH|x y l|l l' l'' P1 IH1 P2 IH2]; cbn [pairwise forallb]; [reflexivity| | |congruence].
  - rewrite IH, (forallb_perm _ _ _ P). reflexivity.
  - rewrite (overlaps_sym y x).
    destruct (overlaps x y), (forallb (fun q => negb (overlaps x q)) l), (forallb (fun q => negb (overlaps y q)) l); reflexivity.
Qed.

Lemma pairwise_app l1 l2 : pairwise (l1 ++ l2) = true -> pairwise l1 = true /\ pairwise l2 = true.
Proof.
  induction l1 as [|r l1 IH]; cbn [pairwise app]; [intros P; split; [reflexivity|exact P]|].
  rewrite !andb_true_iff, forallb_app, andb_true_iff. intros [[F1 F2] P]. apply IH in P as [P1 P2]. tauto.
Qed.

Lemma pw_split l1 x l2 : pairwise (l1 ++ x :: l2) = true -> pairwise (x :: l1) = true /\ pairwise (x :: l2) = true.
Proof.
  intros P. split.
  - rewrite <- (pairwise_perm _ _ (Permutation_middle l1 l2 x)) in P. apply (pairwise_app (x :: l1) l2) in P. tauto.
  - apply pairwise_app in P. tauto.
Qed.

Lemma pairwise_filter {A} (f : A -> rect) g l : pairwise (map f l) = true -> pairwise (map f (filter g l)) = true.
Proof.
  induction l as [|a l IH]; cbn; [auto|]. rewrite andb_true_iff. intros [F P].
  destruct (g a); cbn; [|auto]. rewrite andb_true_iff. split; [|auto].
  rewrite forallb_forall in *. intros q Hq. apply in_map_iff in Hq as [x [<- Hx]]. apply filter_In in Hx as [Hx _].
  apply F, in_map, Hx.
Qed.

Lemma ex_pair (seen : list (rect * rect)) c : existsb (fun p => in_pair p c) seen =
  existsb (fun r => holds r c) (map fst seen) || existsb (fun r => holds r c) (map snd seen).
Proof.
  induction seen as [|p seen IH]; cbn [existsb map]; [reflexivity|]. rewrite IH. unfold in_pair.
  destruct (holds (fst p) c), (holds (snd p) c), (existsb (fun r => holds r c) (map fst seen)),
    (existsb (fun r => holds r c) (map snd seen)); reflexivity.
Qed.

Lemma xor_none (rest : list (rect * rect)) c :
  existsb (fun r => holds r c) (map fst rest) = false -> existsb (fun r => holds r c) (map snd rest) = false ->
  existsb (fun p => xorb (holds (fst p) c) (holds (snd p) c)) rest = false.
Proof.
  induction rest as [|p rest IH]; cbn [map existsb]; [reflexivity|].
  rewrite !orb_false_iff. intros [-> H1] [-> H2]. split; [reflexivity|apply IH; assumption].
Qed.

(* ---------- One step ---------- *)

Lemma bool_fresh a b eo en : (a = true -> eo = false) -> (b = true -> en = false) ->
  ind (a && a) + ind (b && b) - 2 * ind (a && b) - (ind a * ind en + ind b * ind eo) = ind (xorb a b && negb (eo || en)).
Proof.
  destruct a, b, eo, en; intros H1 H2; try reflexivity; try discriminate (H1 eq_refl); discriminate (H2 eq_refl).
Qed.

Lemma fresh_cells o n seen U : NoDup U -> (forall c, holds o c || holds n c = true -> In c U) ->
  pairwise (o :: map fst seen) = true -> pairwise (n :: map snd seen) = true ->
  fresh_term o n seen = count U (fun c => xorb (holds o c) (holds n c) && negb (existsb (fun p => in_pair p c) seen)).
Proof.
  intros N C Po Pn.
  assert (Co : forall c, holds o c = true -> In c U) by (intros c H; apply C; rewrite H; reflexivity).
  assert (Cn : forall c, holds n c = true -> In c U) by (intros c H; apply C; rewrite H, orb_true_r; reflexivity).
  unfold fresh_term. rewrite counted_sum.
  rewrite (sumf_ext _ (fun p => sumf (fun c => ind (holds o c && holds (snd p) c) + ind (holds n c && holds (fst p) c)) U)).
  2: { intros p _. rewrite <- (count_shared U o (snd p)), <- (count_shared U n (fst p)) by assumption.
       rewrite !count_sum, <- sumf_add. reflexivity. }
  rewrite sumf_swap.
  rewrite <- (count_shared U o o), <- (count_shared U n n), <- (count_shared U o n) by assumption.
  rewrite !count_sum, <- sumf_lin. apply sumf_ext. intros c _. cbv beta.
  rewrite (sumf_ext _ (fun p => ind (holds o c) * ind (holds (snd p) c) + ind (holds n c) * ind (holds (fst p) c))).
  2: { intros p _. cbv beta. destruct (holds o c), (holds n c), (holds (snd p) c), (holds (fst p) c); reflexivity. }
  rewrite (sumf_scale (ind (holds o c)) (ind (holds n c)) (fun p => ind (holds (snd p) c)) (fun p => ind (holds (fst p) c))).
  rewrite <- (sumf_map (fun r => ind (holds r c)) snd), <- (sumf_map (fun r => ind (holds r c)) fst).
  pose proof Po as Po'. pose proof Pn as Pn'. cbn [pairwise] in Po', Pn'.
  apply andb_true_iff in Po' as [_ Po']. apply andb_true_iff in Pn' as [_ Pn'].
  rewrite !ind_exists by assumption. rewrite ex_pair. apply bool_fresh; [apply pw_head; exact Po|apply pw_head; exact Pn].
Qed.

(* ---------- The sum ---------- *)

Lemma count_false U : count U (fun _ => false) = 0.
Proof. unfold count. induction U; cbn; auto. Qed.

Lemma bool_step a b e x : (a = true -> b = true -> x = false) ->
  (xorb a b && negb e) || (x && negb (e || (a || b || false))) = (xorb a b || x) && negb e.
Proof. destruct a, b, e, x; intros H; try reflexivity; discriminate (H eq_refl eq_refl). Qed.

Lemma bool_disj a b e x : (xorb a b && negb e) && (x && negb (e || (a || b || false))) = false.
Proof. destruct a, b, e, x; reflexivity. Qed.

Lemma terms_sum U seen moved acc : NoDup U -> (forall p c, In p moved -> in_pair p c = true -> In c U) ->
  pairwise (map fst (seen ++ moved)) = true -> pairwise (map snd (seen ++ moved)) = true ->
  fold_left Z.add (fresh_terms seen moved) acc =
    acc + count U (fun c => existsb (fun p => xorb (holds (fst p) c) (holds (snd p) c)) moved
                            && negb (existsb (fun p => in_pair p c) seen)).
Proof.
  revert seen acc. induction moved as [|[o n] rest IH]; intros seen acc N C Po Pn; cbn [fresh_terms fold_left].
  - rewrite (count_ext U _ (fun _ => false)) by (intros; reflexivity). rewrite count_false. lia.
  - pose proof Po as Po'. pose proof Pn as Pn'. rewrite map_app in Po', Pn'. cbn [map fst snd] in Po', Pn'.
    destruct (pw_split _ _ _ Po') as [Po1 Po2]. destruct (pw_split _ _ _ Pn') as [Pn1 Pn2].
    rewrite IH; [|exact N|intros p c Hp Hc; apply (C p c); [right; exact Hp|exact Hc]
                |rewrite <- app_assoc; exact Po|rewrite <- app_assoc; exact Pn].
    rewrite (fresh_cells o n seen U N) by (exact Po1 || exact Pn1 || (intros c Hc; apply (C (o, n) c); [left; reflexivity|exact Hc])).
    rewrite <- Z.add_assoc, count_or.
    + f_equal. apply count_ext. intros c. cbv beta. rewrite existsb_app. cbn [existsb fst snd].
      change (in_pair (o, n) c) with (holds o c || holds n c).
      apply bool_step. intros A B. apply xor_none; [apply (pw_head _ _ c Po2 A)|apply (pw_head _ _ c Pn2 B)].
    + intros c. cbv beta. rewrite existsb_app. cbn [existsb fst snd].
      change (in_pair (o, n) c) with (holds o c || holds n c). apply bool_disj.
Qed.

(* ---------- Owners ---------- *)

Definition own (ws : list window) c := option_map wid (find (fun w => holds (wrect w) c) ws).

Lemma own_cons w ws c : own (w :: ws) c = if holds (wrect w) c then Some (wid w) else own ws c.
Proof. unfold own. cbn [find option_map]. destruct (holds (wrect w) c); reflexivity. Qed.

Lemma own_some ws c k : own ws c = Some k -> In k (ids ws).
Proof.
  unfold own, ids. destruct (find (fun w => holds (wrect w) c) ws) eqn:F; cbn; intros E; inversion E; subst.
  apply find_some in F as [F _]. apply in_map, F.
Qed.

Lemma ids_relayout ws cur k : In k (ids (relayout ws cur)) -> In k (ids ws).
Proof.
  unfold ids, relayout. rewrite map_map. intros H. apply in_map_iff in H as [[w r] [<- Hp]]. cbn.
  apply in_map. exact (in_combine_l _ _ _ _ Hp).
Qed.

Lemma combine_fst_snd {A B} (l : list A) (l' : list B) : length l = length l' ->
  map fst (combine l l') = l /\ map snd (combine l l') = l'.
Proof.
  revert l'. induction l as [|a l IH]; intros [|b l'] L; cbn in *; try discriminate; [split; reflexivity|].
  rewrite (proj1 (IH l' ltac:(lia))), (proj2 (IH l' ltac:(lia))). split; reflexivity.
Qed.

Lemma rects_relayout ws cur : length cur = length ws -> map wrect (relayout ws cur) = cur.
Proof.
  revert cur. induction ws as [|w ws IH]; intros [|r cur] L; try discriminate L; [reflexivity|].
  change (relayout (w :: ws) (r :: cur)) with (set_rect r w :: relayout ws cur). cbn [map wrect set_rect].
  f_equal. apply IH. cbn in L. lia.
Qed.

Lemma rect_eqb_eq a b : rect_eqb a b = true -> a = b.
Proof.
  destruct a, b. unfold rect_eqb, span_eq. cbn. rewrite !andb_true_iff, !Z.eqb_eq.
  intros [[-> ->] [-> ->]]. reflexivity.
Qed.

Lemma exists_moved (l : list (rect * rect)) c :
  existsb (fun p => xorb (holds (fst p) c) (holds (snd p) c)) (filter (fun p => negb (rect_eqb (fst p) (snd p))) l) =
  existsb (fun p => xorb (holds (fst p) c) (holds (snd p) c)) l.
Proof.
  induction l as [|p l IH]; cbn [filter existsb]; [reflexivity|].
  destruct (rect_eqb (fst p) (snd p)) eqn:E; cbn [negb existsb]; rewrite IH.
  - rewrite (rect_eqb_eq _ _ E), xorb_nilpotent. reflexivity.
  - reflexivity.
Qed.

Lemma owner_change ws cur c : NoDup (ids ws) -> length cur = length ws ->
  pairwise (map wrect ws) = true -> pairwise cur = true ->
  negb (same_owner (owner ws (fst c) (snd c)) (owner (relayout ws cur) (fst c) (snd c))) =
  existsb (fun p => xorb (holds (fst p) c) (holds (snd p) c)) (combine (map wrect ws) cur).
Proof.
  change (owner ws (fst c) (snd c)) with (own ws c).
  change (owner (relayout ws cur) (fst c) (snd c)) with (own (relayout ws cur) c).
  revert cur. induction ws as [|w ws IH]; intros [|r cur] N L Pw Pc; try discriminate L; [reflexivity|].
  change (relayout (w :: ws) (r :: cur)) with (set_rect r w :: relayout ws cur).
  rewrite !own_cons. cbn [map combine existsb fst snd wrect wid set_rect].
  pose proof (pw_head _ _ c Pw) as Hw. pose proof (pw_head _ _ c Pc) as Hr.
  cbn [pairwise map] in Pw, Pc. apply andb_true_iff in Pw as [_ Pw]. apply andb_true_iff in Pc as [_ Pc].
  cbn [ids map] in N. apply NoDup_cons_iff in N as [Nw N'].
  assert (Lw : length (map wrect ws) = length cur) by (rewrite length_map; cbn in L; lia).
  destruct (combine_fst_snd _ _ Lw) as [F1 F2].
  destruct (holds (wrect w) c) eqn:A, (holds r c) eqn:B; cbn [xorb orb].
  - cbn [same_owner]. rewrite Nat.eqb_refl. symmetry. apply xor_none; [rewrite F1|rewrite F2]; [apply Hw|apply Hr]; reflexivity.
  - destruct (own (relayout ws cur) c) as [k|] eqn:O; [|reflexivity]. cbn [same_owner].
    apply own_some, ids_relayout in O. destruct (Nat.eqb_spec (wid w) k); [subst; exact (False_ind _ (Nw O))|reflexivity].
  - destruct (own ws c) as [k|] eqn:O; [|reflexivity]. cbn [same_owner].
    apply own_some in O. destruct (Nat.eqb_spec k (wid w)); [subst; exact (False_ind _ (Nw O))|reflexivity].
  - apply IH; [exact N'|cbn in L; lia|exact Pw|exact Pc].
Qed.

Lemma bbox_in L x0 y0 x1 y1 : bbox L = (x0, y0, x1, y1) -> forall w, In w L ->
  x0 <= ri (wrect w) /\ right (wrect w) <= x1 /\ y0 <= rj (wrect w) /\ bottom (wrect w) <= y1.
Proof.
  unfold bbox. revert x0 y0 x1 y1. induction L as [|w L IH]; cbn [fold_right]; intros x0 y0 x1 y1 B q Hq; [destruct Hq|].
  destruct (fold_right _ _ L) as [[[a b] c] d] eqn:E. injection B as <- <- <- <-.
  destruct Hq as [<-|Hq]; [lia|]. specialize (IH a b c d eq_refl q Hq). lia.
Qed.

(* One step: over any list of distinct tiles holding o's and n's, the term counts the tiles of o
   or n but not both that no earlier pair holds, when the old rectangles are disjoint and so are
   the new ones. *)
Theorem fresh_term_cells (o n : rect) (seen : list (rect * rect)) (U : list (Z * Z)) :
  NoDup U -> (forall c, holds o c || holds n c = true -> In c U) ->
  pairwise (o :: map fst seen) = true -> pairwise (n :: map snd seen) = true ->
  0 <= rw o -> 0 <= rh o -> 0 <= rw n -> 0 <= rh n ->
  Forall (fun p => 0 <= rw (fst p) /\ 0 <= rh (fst p) /\ 0 <= rw (snd p) /\ 0 <= rh (snd p)) seen ->
  fresh_term o n seen =
    count U (fun c => xorb (holds o c) (holds n c) && negb (existsb (fun p => in_pair p c) seen)).
Proof.
  intros N C Po Pn _ _ _ _ _. exact (fresh_cells o n seen U N C Po Pn).
Qed.

(* The terms sum to rule 1's tiles reassigned when nothing overlaps before or after. *)
Theorem tiles_displaced (ws : list window) (cur : list rect) :
  NoDup (ids ws) -> spans_ok ws -> length cur = length ws ->
  Forall (fun r => 0 <= rw r /\ 0 <= rh r) cur ->
  pairwise (map wrect ws) = true -> pairwise cur = true ->
  fold_left Z.add (fresh_terms [] (moved_pairs ws cur)) 0 = Z.of_nat (displaced ws (relayout ws cur)).
Proof.
  intros N _ L _ Pw Pc. unfold displaced.
  destruct (bbox (ws ++ relayout ws cur)) as [[[x0 y0] x1] y1] eqn:B. cbv beta iota.
  assert (Lw : length (map wrect ws) = length cur) by (rewrite length_map; lia).
  destruct (combine_fst_snd _ _ Lw) as [F1 F2].
  pose proof (rects_relayout ws cur L) as W.
  rewrite (terms_sum (grid x0 x1 y0 y1) [] (moved_pairs ws cur) 0).
  - rewrite Z.add_0_l. unfold count. do 2 f_equal. apply filter_ext. intros c. cbv beta.
    cbn [existsb negb]. rewrite andb_true_r. unfold moved_pairs. rewrite exists_moved.
    symmetry. apply owner_change; assumption.
  - apply nodup_grid.
  - intros [o n] c Hp Hc. unfold moved_pairs in Hp. apply filter_In in Hp as [Hp _].
    assert (G : forall w, In w (ws ++ relayout ws cur) -> holds (wrect w) c = true -> In c (grid x0 x1 y0 y1)).
    { intros w Hw Hh. destruct (bbox_in _ _ _ _ _ B w Hw) as (H1 & H2 & H3 & H4). apply in_grid.
      unfold holds in Hh. rewrite !andb_true_iff, !Z.leb_le, !Z.ltb_lt in Hh. lia. }
    unfold in_pair in Hc. cbn [fst snd] in Hc. apply orb_true_iff in Hc as [Hc|Hc].
    + apply in_combine_l, in_map_iff in Hp as [w [<- Hw]]. apply (G w); [apply in_or_app; left|]; assumption.
    + apply in_combine_r in Hp. rewrite <- W in Hp. apply in_map_iff in Hp as [w [<- Hw]].
      apply (G w); [apply in_or_app; right|]; assumption.
  - unfold moved_pairs. cbn [app]. apply pairwise_filter. rewrite F1. exact Pw.
  - unfold moved_pairs. cbn [app]. apply pairwise_filter. rewrite F2. exact Pc.
Qed.
