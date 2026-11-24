(* F01's repair: the sweep sorts the windows by (lead, id), so a window that lies before another
   along d is swept first, and the later one stays past it when their cross ranges overlap. *)
From Stdlib Require Import ZArith List Bool Lia Permutation Sorted.
Import ListNotations.
From NewmCanvas Require Import Canvas Push Claims.
Open Scope Z_scope.

Lemma sooner_iff d a b : sooner d a b = true <->
  lead d (wrect a) < lead d (wrect b) \/ lead d (wrect a) = lead d (wrect b) /\ (wid a < wid b)%nat.
Proof. unfold sooner. rewrite orb_true_iff, andb_true_iff, Z.ltb_lt, Z.eqb_eq, Nat.ltb_lt. reflexivity. Qed.

Lemma insert_sorted d w l : StronglySorted (fun a b => sooner d b a = false) l ->
  StronglySorted (fun a b => sooner d b a = false) (insert d w l).
Proof.
  induction l as [|x l IH]; simpl; intros S; [repeat constructor|].
  apply StronglySorted_inv in S as [S F]. rewrite Forall_forall in F. destruct (sooner d x w) eqn:E.
  - constructor; [exact (IH S)|]. apply Forall_forall. intros y Hy.
    apply (Permutation_in _ (Permutation_sym (insert_perm d w l))) in Hy as [<-|Hy]; [|exact (F y Hy)].
    apply sooner_iff in E. rewrite <- not_true_iff_false, sooner_iff. lia.
  - constructor; [constructor; [exact S|apply Forall_forall; exact F]|]. constructor; [exact E|].
    apply Forall_forall. intros y Hy. specialize (F y Hy). rewrite <- not_true_iff_false, sooner_iff in *. lia.
Qed.

Lemma order_sorted d m ws : StronglySorted (fun a b => sooner d b a = false) (order d m ws).
Proof. unfold order. induction (filter _ ws); simpl; [constructor|apply insert_sorted; assumption]. Qed.

Lemma sweep_order d mr swept order a b x y : NoDup (ids order) -> StronglySorted (fun a b => sooner d b a = false) order ->
  In a order -> In b order -> In x (sweep d mr swept order) -> In y (sweep d mr swept order) ->
  wid x = wid a -> wid y = wid b -> sooner d a b = true -> clear d (wrect x) (wrect y).
Proof.
  revert swept; induction order as [|w o IH]; intros swept Hn Ss Ha Hb Hx Hy Ea Eb L; [contradiction|].
  cbn [sweep] in Hx, Hy. pose proof Hn as Hn0. simpl in Hn. inversion Hn as [|? ? Hw Hn']; subst.
  apply StronglySorted_inv in Ss as [Ss F]. rewrite Forall_forall in F.
  set (r := settle d mr swept (S (length swept)) (wrect w)) in *.
  assert (Tail : forall c z, In c (w :: o) -> In z (sweep d mr (swept ++ [r]) o) -> wid z = wid c -> In c o).
  { intros c z [<-|Hc] Hz Ez; [|exact Hc]. exfalso. apply Hw. rewrite <- Ez, <- (sweep_ids d mr (swept ++ [r])).
    apply in_map, Hz. }
  destruct Hx as [<-|Hx], Hy as [<-|Hy].
  - assert (a = b) as -> by (apply (nodup_unique (w :: o)); simpl in *; auto; congruence).
    apply sooner_iff in L. lia.
  - apply (proj2 (sweep_clear _ _ _ _ _ Hy)). apply in_or_app; simpl; auto.
  - assert (b = w) as -> by (apply (nodup_unique (w :: o)); simpl in *; auto; congruence).
    specialize (F a (Tail a x Ha Hx Ea)). congruence.
  - exact (IH _ Hn' Ss (Tail a x Ha Hx Ea) (Tail b y Hb Hy Eb) Hx Hy Ea Eb L).
Qed.

(* A push keeps the order along d of every two windows whose cross ranges overlap (C14). *)
Theorem push_keeps_order d m ws ws' : NoDup (ids ws) -> (forall w, In w ws -> 0 < rw (wrect w) /\ 0 < rh (wrect w)) ->
  push d m ws = Some ws' -> keeps_order d m ws ws'.
Proof.
  intros Hn Hs H a b a' b' Ha Hb Ha' Hb' Ea Eb Am Bm P. destruct (Hs a Ha), (Hs b Hb).
  unfold push in H. destruct (rect_of m ws) as [mr|]; injection H as <-.
  2: { assert (a' = a) as -> by (apply (nodup_unique ws); congruence).
       assert (b' = b) as -> by (apply (nodup_unique ws); congruence).
       unfold precedes, follows in *; destruct d; bools; apply Z.leb_le; assumption. }
  apply in_map_iff in Ha' as (a0 & <- & Ha0). apply in_map_iff in Hb' as (b0 & <- & Hb0). rewrite put_wid in Ea, Eb.
  assert (a0 = a) as -> by (apply (nodup_unique ws); congruence). assert (b0 = b) as -> by (apply (nodup_unique ws); congruence).
  destruct (put_other d m mr ws a Hn Ha Am) as (x & Hx & Ex & Ax & ->).
  destruct (put_other d m mr ws b Hn Hb Bm) as (y & Hy & Ey & Ay & ->). simpl.
  assert (Ia : In a (order d m ws)) by (apply order_in; auto). assert (Ib : In b (order d m ws)) by (apply order_in; auto).
  pose proof (fun a b x y => sweep_order d mr [] _ a b x y (order_nodup d m ws Hn) (order_sorted d m ws)) as O.
  pose proof (shares_along d _ _ _ _ Ax Ay) as Sab. pose proof (shares_along d _ _ _ _ Ay Ax) as Sba.
  destruct Ax as (Wa & Ha1 & Ca & _), Ay as (Wb & Hb1 & Cb & _).
  destruct d; [destruct (O b a y x Ib Ia Hy Hx Ey Ex) as [C|C] | destruct (O a b x y Ia Ib Hx Hy Ex Ey) as [C|C]
             | destruct (O b a y x Ib Ia Hy Hx Ey Ex) as [C|C] | destruct (O a b x y Ia Ib Hx Hy Ex Ey) as [C|C]];
    unfold precedes, follows, shares, rows_overlap, cols_overlap, far, lead, right, bottom in *; bools;
    try (apply sooner_iff; unfold lead, right, bottom; lia); try (apply Z.leb_le; lia); exfalso;
    rewrite ?Sab, ?Sba in C; bools; apply andb_false_iff in C as [C|C]; bools; lia.
Qed.

(* C32, C33: move_by moves the window exactly one tile along d, and push keeps the order along d
   of every two other windows whose cross ranges overlap. *)
Theorem move_by_moves_one_tile e c i d p c' w : window_of i (windows c) = Some w -> move_by e c i d p = Some c' ->
  rect_of i (windows c') = Some (shifted (wrect w) (fst (delta d)) (snd (delta d))).
Proof.
  intros Hw H. unfold move_by in H. rewrite Hw in H. destruct (placed_by _ _ p) as [ws|] eqn:Pl.
  { injection H as <-. rewrite (proj1 (efv_frame _ _)). simpl. destruct (placed_layouts_keep_the_rules _ _ _ _ Pl) as (A & _).
    pose proof (f2_window_of _ _ _ i (fun x y H => proj1 H) A) as F. rewrite Hw in F.
    unfold rect_of. destruct (window_of i ws) as [b|]; [|contradiction]. destruct F as (_ & _ & F).
    rewrite (proj2 (window_of_some _ _ _ Hw)) in F. unfold target_of in F. simpl in F. rewrite Nat.eqb_refl in F. simpl in F.
    simpl. rewrite F. reflexivity. }
  unfold move_by_push in H. rewrite Hw in H.
  set (r := shifted _ _ _) in H. set (ws1 := update i (set_rect r) (windows c)) in H.
  destruct (push d i ws1) as [ws|] eqn:Hp; [|discriminate]. injection H as <-. rewrite (proj1 (efv_frame _ _)). simpl.
  assert (W : window_of i ws1 = Some (set_rect r w)) by exact (window_of_update i (set_rect r) _ w (fun _ => eq_refl) Hw).
  unfold push, rect_of in Hp. rewrite W in Hp. simpl in Hp. injection Hp as <-. set (done := sweep d r [] (order d i ws1)).
  pose proof (f2_window_of (fun x y => y = put done x) ws1 (map (put done) ws1) i (fun x y E => ltac:(cbv beta in E; subst; symmetry; apply put_wid)) (f2_map _ _ _ (fun x _ => eq_refl))) as F.
  rewrite W in F. unfold rect_of. destruct (window_of i (map (put done) ws1)) as [b|]; [|contradiction]. subst b.
  unfold done. rewrite put_mover; [reflexivity|exact (proj2 (window_of_some _ _ _ Hw))].
Qed.

Theorem move_by_keeps_order e c i d c' : NoDup (ids (windows c)) -> (forall w, In w (windows c) -> 0 < rw (wrect w) /\ 0 < rh (wrect w)) ->
  move_by e c i d None = Some c' -> keeps_order d i (windows c) (windows c').
Proof.
  intros Hn Hs H. unfold move_by in H. destruct (window_of i (windows c)) as [w|] eqn:Hw.
  2: { injection H as <-. apply (push_keeps_order d i _ _ Hn Hs). unfold push, rect_of. rewrite Hw. reflexivity. }
  cbn [placed_by] in H. unfold move_by_push in H. rewrite Hw in H.
  set (r := shifted _ _ _) in H. set (ws1 := update i (set_rect r) (windows c)) in H.
  destruct (push d i ws1) as [ws|] eqn:Hp; [|discriminate]. injection H as <-. rewrite (proj1 (efv_frame _ _)). simpl.
  assert (U : forall a, In a (windows c) -> wid a <> i -> In a ws1).
  { intros a Ha E. unfold ws1, update. apply in_map_iff. exists a. apply Nat.eqb_neq in E. rewrite E. auto. }
  assert (Hs1 : forall x, In x ws1 -> 0 < rw (wrect x) /\ 0 < rh (wrect x)).
  { intros x Hx. apply in_map_iff in Hx as (y & <- & Hy). destruct (Nat.eqb (wid y) i); [exact (Hs w (proj1 (window_of_some _ _ _ Hw)))|exact (Hs y Hy)]. }
  intros a b a' b' Ha Hb Ha' Hb' Ea Eb Am Bm.
  exact (push_keeps_order d i ws1 ws ltac:(unfold ws1; rewrite update_ids; auto) Hs1 Hp a b a' b' (U a Ha Am) (U b Hb Bm) Ha' Hb' Ea Eb Am Bm).
Qed.
