(* The claims about every public operation. *)
From Stdlib Require Import ZArith List Bool Lia Permutation.
Import ListNotations.
From NewmCanvas Require Import Canvas Push.
Open Scope Z_scope.

Definition inv e c := NoDup (ids (windows c)) /\ disjoint (windows c) /\ mins_ok e (windows c).
Definition focus_ok e c := forall r, focus_rect c = Some r ->
  fits e (zoom c) (screen c) (rw r) (rh r) = true -> is_visible e c r = true.

Lemma efv_frame e c : windows (ensure_focus_visible e c) = windows c /\ focus (ensure_focus_visible e c) = focus c /\
  zoom (ensure_focus_visible e c) = zoom c /\ screen (ensure_focus_visible e c) = screen c /\
  recent (ensure_focus_visible e c) = recent c.
Proof. unfold ensure_focus_visible, ensure_visible. destruct (focus_rect c); simpl; auto. Qed.
Ltac efv := repeat match goal with |- context [ensure_focus_visible ?e ?x] =>
  destruct (efv_frame e x) as (?E1 & ?E2 & ?E3 & ?E4 & ?E5); progress rewrite ?E1, ?E2, ?E3, ?E4, ?E5 end.

Lemma inv_efv e c : inv e c -> inv e (ensure_focus_visible e c).
Proof. destruct (efv_frame e c) as (E1 & _). unfold inv; rewrite E1; auto. Qed.

Lemma mins_spans e ws : good e -> mins_ok e ws -> spans_ok ws.
Proof. intros (_ & _ & Hg) Hm w Hw. destruct (Hm w Hw). destruct (Hg (wmin w)). lia. Qed.

Lemma f2_impl (R S : window -> window -> Prop) l1 l2 :
  (forall x y, In x l1 -> R x y -> S x y) -> Forall2 R l1 l2 -> Forall2 S l1 l2.
Proof.
  intros H A; induction A; constructor; [apply H; [left; reflexivity|assumption]|].
  apply IHA; intros; apply H; [right|]; assumption.
Qed.

Lemma update_regrown m r' ws : Forall2 (regrown m r') ws (update m (set_rect r') ws).
Proof.
  apply update_f2; intros w _ E; unfold regrown; simpl; repeat split.
  - rewrite E, Nat.eqb_refl. auto.
  - destruct (Nat.eqb (wid w) m) eqn:E'; bools; [congruence|auto].
Qed.
Lemma regrown_mins e m r' w ws ws' : NoDup (ids ws) -> In w ws -> wid w = m -> mins_ok e ws ->
  fst (need e (wmin w)) <= rw r' -> snd (need e (wmin w)) <= rh r' -> Forall2 (regrown m r') ws ws' -> mins_ok e ws'.
Proof.
  intros Hn Hw Ew Hm H1 H2 A y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (E1 & E2 & E3)).
  rewrite <- E2. destruct (Nat.eqb (wid x) m) eqn:E; bools; destruct E3 as [-> ->].
  - assert (x = w) as -> by (apply (nodup_unique ws); congruence). auto.
  - apply Hm; exact Hx.
Qed.

Lemma without_ok i ws : NoDup (ids ws) -> NoDup (ids (without i ws)) /\ ~ In i (ids (without i ws)) /\
  forall x, In x (without i ws) -> In x ws.
Proof.
  intros Hn. split; [|split].
  - unfold without, ids. induction ws as [|w ws IH]; simpl in *; [constructor|].
    inversion Hn as [|? ? Hw Hn']; subst. destruct (negb (Nat.eqb (wid w) i)); simpl; auto.
    constructor; auto. intros H. apply Hw. apply in_map_iff in H as (x & <- & Hx).
    apply filter_In in Hx as [Hx _]. apply in_map; exact Hx.
  - unfold without, ids. intros H. apply in_map_iff in H as (x & E & Hx). apply filter_In in Hx as [_ Hx].
    rewrite E, Nat.eqb_refl in Hx. discriminate.
  - intros x Hx. apply filter_In in Hx as [Hx _]. exact Hx.
Qed.

Lemma closed_ok e freed ws : NoDup (ids ws) -> disjoint ws -> mins_ok e ws ->
  NoDup (ids (closed freed ws)) /\ disjoint (closed freed ws) /\ mins_ok e (closed freed ws) /\ Forall2 same ws (closed freed ws).
Proof.
  intros Hn Hd Hm. unfold closed.
  assert (A : Forall2 same ws (fill freed Down (fill freed Right ws))) by (eapply f2_same_trans; apply fill_same).
  split; [rewrite <- (same_ids _ _ A); exact Hn|]. split; [apply fill_disjoint, fill_disjoint, Hd|].
  split; [exact (same_mins _ _ _ A Hm)|exact A].
Qed.

Lemma take_ok e i ws w rest : NoDup (ids ws) -> disjoint ws -> mins_ok e ws -> take i ws = Some (w, rest) ->
  window_of i ws = Some w /\ NoDup (ids rest) /\ ~ In i (ids rest) /\ disjoint rest /\ mins_ok e rest.
Proof.
  intros Hn Hd Hm H. unfold take in H. destruct (window_of i ws) as [w0|] eqn:Hw; [|discriminate].
  injection H as <- <-. destruct (without_ok i ws Hn) as (N1 & N2 & N3).
  destruct (closed_ok e (wrect w0) (without i ws) N1 (fun a b Ha Hb => Hd a b (N3 a Ha) (N3 b Hb))
              (fun x Hx => Hm x (N3 x Hx))) as (C1 & C2 & C3 & C4).
  split; [reflexivity|split; [exact C1|split; [|split; [exact C2|exact C3]]]].
  rewrite <- (same_ids _ _ C4). exact N2.
Qed.

Lemma disjoint_snoc l w : disjoint l -> (forall x, In x l -> overlaps (wrect x) (wrect w) = false) -> disjoint (l ++ [w]).
Proof.
  intros Hd Hw a b Ha Hb Hab. apply in_app_or in Ha as [Ha|[<-|[]]]; apply in_app_or in Hb as [Hb|[<-|[]]].
  - apply Hd; auto.
  - apply Hw; auto.
  - rewrite overlaps_sym; apply Hw; auto.
  - congruence.
Qed.

(* enforce_min widens and heightens one window to its need, and nothing else changes size. *)
(* The engine's and gather's answers as place and gather check them, and restore's snapshot. *)
Lemma combine_f2 (R : window -> window -> Prop) ws l : length l = length ws ->
  (forall p, In p (combine ws l) -> R (fst p) (set_rect (snd p) (fst p))) -> Forall2 R ws (relayout ws l).
Proof.
  unfold relayout. revert l. induction ws as [|w ws IH]; intros [|r l] E H; simpl in *; try discriminate; constructor.
  - apply (H (w, r)); left; reflexivity.
  - apply IH; [lia|]. intros q Hq. apply H. right. exact Hq.
Qed.
Lemma relayout_rects ws l : length l = length ws -> map wrect (relayout ws l) = l.
Proof.
  unfold relayout. revert l. induction ws as [|w ws IH]; intros [|r l] E; simpl in *; try discriminate; [reflexivity|].
  f_equal. apply IH. lia.
Qed.
Lemma pairwise_disjoint ws : pairwise (map wrect ws) = true -> disjoint ws.
Proof.
  induction ws as [|x ws IH]; simpl; intros H a b Ha Hb Hab; [contradiction|].
  apply andb_true_iff in H as [H1 H2]. rewrite forallb_forall in H1.
  destruct Ha as [<-|Ha], Hb as [<-|Hb]; [congruence| | |exact (IH H2 a b Ha Hb Hab)].
  - apply negb_true_iff, H1, in_map, Hb.
  - rewrite overlaps_sym. apply negb_true_iff, H1, in_map, Ha.
Qed.
Lemma rect_eqb_eq a b : rect_eqb a b = true -> a = b.
Proof. unfold rect_eqb, span_eq. destruct a, b; simpl. intros H. bools. f_equal; lia. Qed.
Definition placed_as ts (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  match target_of (wid a) ts with Some t => wrect b = t | None => rw (wrect b) = rw (wrect a) /\ rh (wrect b) = rh (wrect a) end.
(* Rule 2 for placed layouts: a layout place accepts from the engine puts each target on its
   rectangle, keeps every other span, overlaps nothing and splits no island. *)
Theorem placed_layouts_keep_the_rules ws ts p ws' : placed_by ws ts p = Some ws' ->
  Forall2 (placed_as ts) ws ws' /\ disjoint ws' /\ splits ws ws' (map fst ts) = false.
Proof.
  unfold placed_by. destruct p as [l|]; [|discriminate]. destruct (accepts ws ts l) eqn:A; [|discriminate].
  intros H. injection H as <-. unfold accepts in A. apply andb_true_iff in A as [A S]. apply andb_true_iff in A as [A P].
  apply andb_true_iff in A as [L F]. apply Nat.eqb_eq in L. apply negb_true_iff in S. rewrite forallb_forall in F.
  split; [|split; [apply pairwise_disjoint; rewrite relayout_rects; auto|exact S]].
  apply combine_f2; [exact L|]. intros [w r] Hp. specialize (F _ Hp). simpl in *. unfold placed_as; simpl.
  split; [reflexivity|split; [reflexivity|]]. destruct (target_of (wid w) ts); [apply rect_eqb_eq; exact F|].
  unfold span_eq in F. bools. split; lia.
Qed.
Lemma target_in x ts t : target_of x ts = Some t -> In (x, t) ts.
Proof.
  unfold target_of. destruct (find _ ts) as [[y u]|] eqn:F; simpl; [|discriminate]. intros E. injection E as <-.
  apply find_some in F as [F E]. simpl in E. apply Nat.eqb_eq in E. subst. exact F.
Qed.
Lemma placed_ids ws ts p ws' : placed_by ws ts p = Some ws' -> ids ws' = ids ws.
Proof. intros H. symmetry. exact (f2_ids _ _ _ (fun x y H => proj1 H) (proj1 (placed_layouts_keep_the_rules _ _ _ _ H))). Qed.
Lemma placed_same ws ts p ws' : placed_by ws ts p = Some ws' ->
  (forall w t, In w ws -> In (wid w, t) ts -> rw t = rw (wrect w) /\ rh t = rh (wrect w)) -> Forall2 same ws ws'.
Proof.
  intros H T. destruct (placed_layouts_keep_the_rules _ _ _ _ H) as (A & _). revert A. apply f2_impl.
  intros a b Ha (E1 & E2 & E3). unfold same. destruct (target_of (wid a) ts) as [r|] eqn:Ht.
  - destruct (T a r Ha (target_in _ _ _ Ht)) as [T1 T2]. rewrite E3. repeat split; congruence.
  - destruct E3 as [E3 E4]. repeat split; congruence.
Qed.
Lemma placed_same_inv e ws ts p ws' : placed_by ws ts p = Some ws' -> NoDup (ids ws) -> mins_ok e ws ->
  (forall w t, In w ws -> In (wid w, t) ts -> rw t = rw (wrect w) /\ rh t = rh (wrect w)) ->
  NoDup (ids ws') /\ disjoint ws' /\ mins_ok e ws'.
Proof.
  intros H Hn Hm T. pose proof (placed_same _ _ _ _ H T) as A.
  split; [rewrite <- (same_ids _ _ A); exact Hn|split; [exact (proj1 (proj2 (placed_layouts_keep_the_rules _ _ _ _ H)))|exact (same_mins _ _ _ A Hm)]].
Qed.
Lemma one_target ws i w t : NoDup (ids ws) -> window_of i ws = Some w -> rw t = rw (wrect w) -> rh t = rh (wrect w) ->
  forall x u, In x ws -> In (wid x, u) [(i, t)] -> rw u = rw (wrect x) /\ rh u = rh (wrect x).
Proof.
  intros Hn Hw E1 E2 x u Hx [E|[]]. injection E as E3 E4. subst u.
  apply window_of_some in Hw as [Hw Hi]. assert (x = w) as -> by (apply (nodup_unique ws); auto; congruence). split; assumption.
Qed.
Lemma placed_same_at ws i t p ws' : placed_by ws [(i, t)] p = Some ws' -> Forall2 (same_at i) (update i (set_rect t) ws) ws'.
Proof.
  intros H. destruct (placed_layouts_keep_the_rules _ _ _ _ H) as (A & _). unfold update. clear H. revert ws' A.
  induction ws as [|w ws IH]; intros l A; inversion A as [|x y l1 l2 B C]; subst; constructor; [|apply IH, C].
  destruct B as (B1 & B2 & B3). unfold target_of in B3. simpl in B3. rewrite (Nat.eqb_sym i (wid w)) in B3.
  unfold same_at. destruct (Nat.eqb (wid w) i); simpl in *.
  - split; [exact B1|split; [exact B2|intros _; rewrite B3; split; reflexivity]].
  - destruct B3 as [B3 B4]. split; [exact B1|split; [exact B2|intros _; split; congruence]].
Qed.
Lemma gathered_ok ws g : Forall2 same ws (gathered ws g) /\ (disjoint ws -> disjoint (gathered ws g)).
Proof.
  unfold gathered. destruct g as [l|]; [|split; [apply f2_same_refl|auto]].
  destruct (_ && _ && _) eqn:A; [|split; [apply f2_same_refl|auto]].
  apply andb_true_iff in A as [A P]. apply andb_true_iff in A as [L F]. apply Nat.eqb_eq in L. rewrite forallb_forall in F.
  split; [|intros _; apply pairwise_disjoint; rewrite relayout_rects; auto].
  apply combine_f2; [exact L|]. intros [w r] Hp. specialize (F _ Hp). simpl in *. unfold span_eq in F. bools.
  unfold same; simpl. repeat split; lia.
Qed.
Lemma gathered_inv e ws g : NoDup (ids ws) -> disjoint ws -> mins_ok e ws ->
  NoDup (ids (gathered ws g)) /\ disjoint (gathered ws g) /\ mins_ok e (gathered ws g).
Proof.
  intros Hn Hd Hm. destruct (gathered_ok ws g) as [A D].
  split; [rewrite <- (same_ids _ _ A); exact Hn|split; [exact (D Hd)|exact (same_mins _ _ _ A Hm)]].
Qed.
Lemma gathered_ids ws g : ids (gathered ws g) = ids ws.
Proof. symmetry. exact (same_ids _ _ (proj1 (gathered_ok ws g))). Qed.
Lemma closing_same ws i freed g : Forall2 same (without i ws) (closing ws i freed g) /\
  (disjoint (without i ws) -> disjoint (closing ws i freed g)).
Proof.
  unfold closing, closed. cbv zeta. generalize (without i ws) as o. intros o.
  assert (B : forall b : bool, Forall2 same o (if b then o else fill freed Down (fill freed Right o)) /\
    (disjoint o -> disjoint (if b then o else fill freed Down (fill freed Right o)))).
  { intros [|]; [split; [apply f2_same_refl|auto]|split; [eapply f2_same_trans; apply fill_same|intros D; apply fill_disjoint, fill_disjoint, D]]. }
  destruct (B (splits ws (fill freed Down (fill freed Right o)) [i])) as [B1 B2]. destruct (is_bridge ws i); [|exact (conj B1 B2)].
  destruct (gathered_ok (if splits ws (fill freed Down (fill freed Right o)) [i] then o else fill freed Down (fill freed Right o)) g) as [G1 G2].
  split; [exact (f2_same_trans _ _ _ B1 G1)|intros D; apply G2, B2, D].
Qed.
Lemma closing_ids ws i freed g : ids (closing ws i freed g) = ids (without i ws).
Proof. symmetry. exact (same_ids _ _ (proj1 (closing_same ws i freed g))). Qed.
Lemma restored_ok ws l ws' : restored ws l = Some ws' ->
  Forall2 (fun a b => wid a = wid b /\ wmin a = wmin b) ws ws' /\ spans_ok ws' /\ disjoint ws'.
Proof.
  unfold restored. destruct (_ && _ && _ && _) eqn:A; [|discriminate]. intros H. injection H as <-.
  apply andb_true_iff in A as [A P]. apply andb_true_iff in A as [_ S]. rewrite forallb_forall in S.
  split; [|split; [intros w Hw; specialize (S w Hw); bools; split; lia|apply pairwise_disjoint, P]].
  apply f2_map. intros x _. destruct (target_of (wid x) l); simpl; split; reflexivity.
Qed.
Lemma restored_ids ws l ws' : restored ws l = Some ws' -> ids ws' = ids ws.
Proof. intros H. symmetry. exact (f2_ids _ _ _ (fun x y H => proj1 H) (proj1 (restored_ok _ _ _ H))). Qed.

Definition wider e m (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  rw (wrect b) = (if Nat.eqb (wid a) m then Z.max (rw (wrect a)) (fst (need e (wmin a))) else rw (wrect a)) /\
  rh (wrect b) = rh (wrect a).
Definition taller e m (a b : window) := wid a = wid b /\ wmin a = wmin b /\ rw (wrect b) = rw (wrect a) /\
  rh (wrect b) = (if Nat.eqb (wid a) m then Z.max (rh (wrect a)) (snd (need e (wmin a))) else rh (wrect a)).
Definition grew e m (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  rw (wrect b) = (if Nat.eqb (wid a) m then Z.max (rw (wrect a)) (fst (need e (wmin a))) else rw (wrect a)) /\
  rh (wrect b) = (if Nat.eqb (wid a) m then Z.max (rh (wrect a)) (snd (need e (wmin a))) else rh (wrect a)).

(* grow_to: the engine's layout, once place accepts it, ends where the push would on every claim
   used here: the target on its rectangle, every other span kept, nothing overlapping. *)
Lemma grow_ok d m r' ws p ws' : NoDup (ids ws) -> spans_ok ws -> In m (ids ws) -> 0 <= rw r' -> 0 <= rh r' ->
  grow_to ws m r' d p = Some ws' -> NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (regrown m r') ws ws'.
Proof.
  intros Hn Hs Hi Hw Hh. unfold grow_to. destruct (placed_by _ _ p) as [u|] eqn:Pl; [|exact (regrow d m r' ws ws' Hn Hs Hi Hw Hh)].
  intros H. injection H as <-. destruct (placed_layouts_keep_the_rules _ _ _ _ Pl) as (A & D & _).
  assert (R : Forall2 (regrown m r') ws u).
  { revert A. apply f2_impl. intros x y _ (E1 & E2 & E3). unfold regrown. split; [exact E1|split; [exact E2|]].
    unfold target_of in E3. simpl in E3. rewrite (Nat.eqb_sym m (wid x)) in E3.
    destruct (Nat.eqb (wid x) m); simpl in E3; [rewrite E3; split; reflexivity|exact E3]. }
  split; [rewrite (placed_ids _ _ _ _ Pl); exact Hn|split; [|split; [exact D|exact R]]].
  intros y Hy. destruct (f2_in_r _ _ _ _ R Hy) as (x & Hx & (E1 & E2 & E3)).
  destruct (Nat.eqb (wid x) m); destruct E3 as [-> ->]; [split; assumption|apply (Hs x Hx)].
Qed.

Lemma widen_ok e a m ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws -> widen e a m ws = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (wider e m) ws ws'.
Proof.
  intros Hg Hn Hs Hd H. unfold widen in H. destruct (window_of m ws) as [w|] eqn:Hw.
  2: { injection H as <-. split; [exact Hn|split; [exact Hs|split; [exact Hd|]]]. apply f2_diag. intros x Hx.
       unfold wider. destruct (Nat.eqb (wid x) m) eqn:E; bools; [exfalso; exact (window_of_none _ _ Hw x Hx E)|auto]. }
  apply window_of_some in Hw as [Hwin Hwid]. destruct Hg as (_ & _ & Hgn). destruct (Hgn (wmin w)) as [P1 _].
  assert (Own : forall x, In x ws -> wid x = m -> x = w) by (intros x Hx Ex; apply (nodup_unique ws); congruence).
  cbv zeta in H. destruct (rw (wrect w) <? fst (need e (wmin w))) eqn:G; bools.
  - destruct (grow_ok Right m (Rect (ri (wrect w)) (rj (wrect w)) (fst (need e (wmin w))) (rh (wrect w))) _ _ _ Hn Hs ltac:(rewrite <- Hwid; apply in_map, Hwin) ltac:(simpl; lia) ltac:(simpl; apply (Hs w Hwin)) H) as (Hn1 & Hs1 & Hd1 & A).
    split; [exact Hn1|split; [exact Hs1|split; [exact Hd1|]]].
    revert A. apply f2_impl. intros x y Hx (E1 & E2 & E3). unfold wider. split; [exact E1|split; [exact E2|]].
    destruct (Nat.eqb (wid x) m) eqn:E; bools; [|exact E3].
    rewrite (Own x Hx E) in *. simpl in E3. lia.
  - injection H as <-. split; [exact Hn|split; [exact Hs|split; [exact Hd|]]]. apply f2_diag. intros x Hx.
    unfold wider. split; [reflexivity|split; [reflexivity|]].
    destruct (Nat.eqb (wid x) m) eqn:E; bools; [rewrite (Own x Hx E)|]; lia.
Qed.

Lemma heighten_ok e a m ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws -> heighten e a m ws = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (taller e m) ws ws'.
Proof.
  intros Hg Hn Hs Hd H. unfold heighten in H. destruct (window_of m ws) as [w|] eqn:Hw.
  2: { injection H as <-. split; [exact Hn|split; [exact Hs|split; [exact Hd|]]]. apply f2_diag. intros x Hx.
       unfold taller. destruct (Nat.eqb (wid x) m) eqn:E; bools; [exfalso; exact (window_of_none _ _ Hw x Hx E)|auto]. }
  apply window_of_some in Hw as [Hwin Hwid]. destruct Hg as (_ & _ & Hgn). destruct (Hgn (wmin w)) as [_ P2].
  assert (Own : forall x, In x ws -> wid x = m -> x = w) by (intros x Hx Ex; apply (nodup_unique ws); congruence).
  cbv zeta in H. destruct (rh (wrect w) <? snd (need e (wmin w))) eqn:G; bools.
  - destruct (grow_ok Down m (Rect (ri (wrect w)) (rj (wrect w)) (rw (wrect w)) (snd (need e (wmin w)))) _ _ _ Hn Hs ltac:(rewrite <- Hwid; apply in_map, Hwin) ltac:(simpl; apply (Hs w Hwin)) ltac:(simpl; lia) H) as (Hn1 & Hs1 & Hd1 & A).
    split; [exact Hn1|split; [exact Hs1|split; [exact Hd1|]]].
    revert A. apply f2_impl. intros x y Hx (E1 & E2 & E3). unfold taller. split; [exact E1|split; [exact E2|]].
    destruct (Nat.eqb (wid x) m) eqn:E; bools; [|lia].
    rewrite (Own x Hx E) in *. simpl in E3. lia.
  - injection H as <-. split; [exact Hn|split; [exact Hs|split; [exact Hd|]]]. apply f2_diag. intros x Hx.
    unfold taller. split; [reflexivity|split; [reflexivity|]].
    destruct (Nat.eqb (wid x) m) eqn:E; bools; [rewrite (Own x Hx E)|]; lia.
Qed.

Lemma enforce_min_ok e a m ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws ->
  enforce_min e a m ws = Some ws' -> NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (grew e m) ws ws'.
Proof.
  intros Hg Hn Hs Hd H. unfold enforce_min in H. destruct (widen e a m ws) as [ws1|] eqn:H1; [|discriminate].
  destruct (widen_ok _ _ _ _ _ Hg Hn Hs Hd H1) as (Hn1 & Hs1 & Hd1 & A1).
  destruct (heighten_ok _ _ _ _ _ Hg Hn1 Hs1 Hd1 H) as (Hn2 & Hs2 & Hd2 & A2).
  split; [exact Hn2|split; [exact Hs2|split; [exact Hd2|]]].
  revert A1 A2. apply f2_trans. intros x y u (E1 & E2 & E3 & E4) (F1 & F2 & F3 & F4). unfold grew.
  rewrite <- E1, <- E2 in *. split; [congruence|split; [congruence|]]. destruct (Nat.eqb (wid x) m); lia.
Qed.

Lemma grew_mins e m (P : nat -> Prop) l1 l2 : Forall2 (grew e m) l1 l2 ->
  (forall x, In x l1 -> wid x <> m -> P (wid x) ->
     fst (need e (wmin x)) <= rw (wrect x) /\ snd (need e (wmin x)) <= rh (wrect x)) ->
  forall y, In y l2 -> P (wid y) -> fst (need e (wmin y)) <= rw (wrect y) /\ snd (need e (wmin y)) <= rh (wrect y).
Proof.
  intros A H y Hy Py. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (E1 & E2 & E3 & E4)).
  rewrite <- E2, E3, E4. rewrite <- E1 in Py. destruct (Nat.eqb (wid x) m) eqn:E; bools; [lia|]. apply H; auto.
Qed.

Definition grew_all e (P : list nat) (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  rw (wrect b) = (if mem (wid a) P then Z.max (rw (wrect a)) (fst (need e (wmin a))) else rw (wrect a)) /\
  rh (wrect b) = (if mem (wid a) P then Z.max (rh (wrect a)) (snd (need e (wmin a))) else rh (wrect a)).

Lemma enforce_fold e a l P ws0 ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws ->
  Forall2 (grew_all e P) ws0 ws ->
  fold_left (fun acc i => match acc with Some ws => enforce_min e a i ws | None => None end) l (Some ws) = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (grew_all e (P ++ l)) ws0 ws'.
Proof.
  intros Hg. revert P ws. induction l as [|i l IH]; simpl; intros P ws Hn Hs Hd A H.
  - injection H as <-. rewrite app_nil_r. auto.
  - destruct (enforce_min e a i ws) as [ws1|] eqn:E.
    + destruct (enforce_min_ok _ _ _ _ _ Hg Hn Hs Hd E) as (Hn1 & Hs1 & Hd1 & A1).
      replace (P ++ i :: l) with ((P ++ [i]) ++ l) by (rewrite <- app_assoc; reflexivity).
      apply (IH (P ++ [i]) ws1); auto. revert A A1. apply f2_trans. intros x y u (E1 & E2 & E3 & E4) (F1 & F2 & F3 & F4).
      unfold grew_all. rewrite <- E1, <- E2 in *. split; [congruence|split; [congruence|]].
      assert (M : mem (wid x) (P ++ [i]) = mem (wid x) P || Nat.eqb (wid x) i).
      { unfold mem. rewrite existsb_app. simpl. rewrite orb_false_r. reflexivity. }
      rewrite M. destruct (mem (wid x) P), (Nat.eqb (wid x) i); simpl in *; lia.
    + exfalso. clear - H. revert H. induction l; simpl; [discriminate|auto].
Qed.

Lemma enforce_all_ok e a ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws -> enforce_all e a ws = Some ws' ->
  NoDup (ids ws') /\ spans_ok ws' /\ disjoint ws' /\ Forall2 (grew_all e (ids ws)) ws ws'.
Proof.
  intros Hg Hn Hs Hd H. apply (enforce_fold e a (ids ws) [] ws ws ws' Hg Hn Hs Hd); [|exact H].
  apply f2_diag. intros x _. unfold grew_all. simpl. auto.
Qed.

Lemma grown_within ed r d : d < 0 -> within (grown ed r d) r.
Proof. unfold within; destruct ed; simpl; lia. Qed.

Lemma update_within_disjoint i r' ws w : NoDup (ids ws) -> disjoint ws -> In w ws -> wid w = i -> within r' (wrect w) ->
  disjoint (update i (set_rect r') ws).
Proof.
  intros Hn Hd Hw Ew Hin a' b' Ha Hb Hab. unfold update in Ha, Hb.
  apply in_map_iff in Ha as (a & <- & Ha). apply in_map_iff in Hb as (b & <- & Hb).
  assert (C : forall x, In x ws -> wid x <> i -> overlaps r' (wrect x) = false).
  { intros x Hx Ex. destruct (overlaps r' (wrect x)) eqn:E; [|reflexivity].
    rewrite overlaps_sym in E. apply (overlaps_within _ _ _ Hin) in E. rewrite overlaps_sym in E.
    rewrite (Hd w x Hw Hx ltac:(congruence)) in E. discriminate. }
  destruct (Nat.eqb (wid a) i) eqn:Ea, (Nat.eqb (wid b) i) eqn:Eb; simpl in *; bools; try congruence.
  - apply C; auto.
  - rewrite overlaps_sym. apply C; auto.
  - apply Hd; auto.
Qed.

Lemma update_rects_disjoint i f ws : (forall w, wid (f w) = wid w /\ wrect (f w) = wrect w) -> disjoint ws ->
  disjoint (update i f ws).
Proof.
  intros Hf Hd a' b' Ha Hb Hab. unfold update in Ha, Hb.
  apply in_map_iff in Ha as (a & <- & Ha). apply in_map_iff in Hb as (b & <- & Hb).
  destruct (Nat.eqb (wid a) i), (Nat.eqb (wid b) i); destruct (Hf a), (Hf b);
    rewrite ?H, ?H0, ?H1, ?H2 in *; apply Hd; auto; congruence.
Qed.

Lemma swapped_spans d a b fa nb : swapped d a b = (fa, nb) ->
  rw fa = rw a /\ rh fa = rh a /\ rw nb = rw b /\ rh nb = rh b.
Proof. destruct d; simpl; intros H; injection H as <- <-; simpl; auto. Qed.

(* swap: the focus lands and pushes with n out, n comes back and pushes the other way; both keep
   their spans, and the last push leaves no two windows overlapping (push_disjoint). *)
Lemma swap_ok d f n ra fa nb wn ws ws1 ws2 : NoDup (ids ws) -> spans_ok ws ->
  rect_of f ws = Some ra -> window_of n ws = Some wn -> swapped d ra (wrect wn) = (fa, nb) ->
  push d f (update f (set_rect fa) (without n ws)) = Some ws1 ->
  push (opposite d) n (ws1 ++ [set_rect nb wn]) = Some ws2 ->
  NoDup (ids ws2) /\ disjoint ws2 /\ Forall2 same (without n ws ++ [wn]) ws2 /\
  (forall y, In y (without n ws ++ [wn]) -> In y ws).
Proof.
  intros Hn Hs Ra Wn Sw P1 P2. destruct (swapped_spans _ _ _ _ _ Sw) as (S1 & S2 & S3 & S4).
  destruct (rect_of_some _ _ _ Ra) as (wa & Ha & Ea & Ra'). apply window_of_some in Wn as [Hwn Ewn].
  destruct (without_ok n ws Hn) as (N1 & N2 & N3).
  assert (A0 : Forall2 same (without n ws) (update f (set_rect fa) (without n ws))).
  { apply update_f2; intros z Hz Ez; [|unfold same; auto].
    assert (z = wa) as -> by (apply (nodup_unique ws); auto; congruence).
    unfold same; simpl; rewrite S1, S2, Ra'; auto. }
  assert (A1 : Forall2 same (without n ws) ws1).
  { eapply f2_same_trans; [exact A0|]. refine (push_same _ _ _ _ _ P1). rewrite <- (same_ids _ _ A0). exact N1. }
  assert (I1 : ids ws1 = ids (without n ws)) by (symmetry; exact (same_ids _ _ A1)).
  assert (Hs1 : spans_ok ws1) by (apply (same_spans _ _ A1); intros x Hx; exact (Hs x (N3 x Hx))).
  replace n with (wid (set_rect nb wn)) in P2 by (simpl; exact Ewn).
  destruct (Hs wn Hwn).
  destruct (append_push (opposite d) ws1 (set_rect nb wn) ws2 ltac:(rewrite I1; exact N1)
              ltac:(simpl; rewrite Ewn, I1; exact N2) Hs1 ltac:(simpl; lia) ltac:(simpl; lia) P2) as (Hn2 & _ & Hd2 & A2).
  split; [exact Hn2|split; [exact Hd2|split]].
  - eapply f2_same_trans; [|exact A2]. apply Forall2_app; [exact A1|]. constructor; [|constructor].
    unfold same; simpl; auto.
  - intros y Hy. apply in_app_or in Hy as [Hy|[<-|[]]]; auto.
Qed.

Lemma fold_min_le l x0 o : In o l -> fold_right (fun x acc => Z.min (ri (wrect x)) acc) x0 l <= ri (wrect o).
Proof. induction l as [|a l IH]; simpl; [contradiction|]. intros [<-|H]; [lia|specialize (IH H); lia]. Qed.
Lemma fold_max_ge l x0 o : In o l -> right (wrect o) <= fold_right (fun x acc => Z.max (right (wrect x)) acc) x0 l.
Proof. induction l as [|a l IH]; simpl; [contradiction|]. intros [<-|H]; [lia|specialize (IH H); lia]. Qed.

Lemma resize_cases e c i ed d p c' : resize e c i ed d p = Some (true, c') ->
  exists w, window_of i (windows c) = Some w /\ refused e c w (grown ed (wrect w) d) d = false /\
    zoom c' = zoom c /\ screen c' = screen c /\
    ((0 < d /\ (placed_by (windows c) [(i, grown ed (wrect w) d)] p = Some (windows c') \/
        push (outward ed) i (update i (set_rect (grown ed (wrect w) d)) (windows c)) = Some (windows c'))) \/
     (d < 0 /\ exists freed side, strip ed (wrect w) (grown ed (wrect w) d) (- d) = (freed, side) /\
        windows c' = if splits (windows c) (fill freed side (update i (set_rect (grown ed (wrect w) d)) (windows c))) [i]
          then update i (set_rect (grown ed (wrect w) d)) (windows c)
          else fill freed side (update i (set_rect (grown ed (wrect w) d)) (windows c)))).
Proof.
  unfold resize; intros H. destruct (window_of i (windows c)) as [w|] eqn:Hw; [|discriminate]. cbv zeta in H.
  destruct (refused e c w (grown ed (wrect w) d) d) eqn:Rf; [discriminate|].
  exists w. split; [reflexivity|split; [exact Rf|]].
  assert (D0 : d <> 0) by (unfold refused in Rf; destruct (need e (wmin w)); simpl in Rf; bools; lia).
  destruct (0 <? d) eqn:D; bools.
  - destruct (placed_by _ _ p) as [ws|] eqn:Pl.
    + injection H as <-. efv. simpl. split; [reflexivity|split; [reflexivity|]]. left. split; [lia|left; first [exact Pl|reflexivity]].
    + destruct (push _ _ _) as [ws|] eqn:Hp; [|discriminate]. injection H as <-. efv. simpl.
      split; [reflexivity|split; [reflexivity|]]. left. split; [lia|right; first [exact Hp|reflexivity]].
  - destruct (strip ed (wrect w) (grown ed (wrect w) d) (- d)) as [freed side] eqn:St. cbv beta iota in H.
    injection H as <-. efv. simpl. split; [reflexivity|split; [reflexivity|]]. right. split; [lia|]. exists freed, side. split; reflexivity.
Qed.

Lemma refused_false e c w new d : refused e c w new d = false ->
  fst (need e (wmin w)) <= rw new /\ snd (need e (wmin w)) <= rh new /\
  (0 < d -> fits e 0 (screen c) (rw new) (rh new) = true).
Proof.
  unfold refused. destruct (need e (wmin w)) as [nw nh]. simpl. intros H. bools.
  split; [lia|split; [lia|]]. intros Hd. rewrite (proj2 (Z.ltb_lt 0 d) Hd) in *. simpl in *. bools. assumption.
Qed.

(* add appends one window at open_span on a free spot beside the focus and moves no other. *)
Lemma pick_ok e c f d best l r : pick e c f d best l = Some r -> best = Some r \/ (In r l /\ free r [] (windows c) = true).
Proof.
  revert best. induction l as [|x l IH]; cbn [pick]; intros best H; [left; exact H|].
  destruct (IH _ H) as [E|[I Fr]]; [|right; split; [right; exact I|exact Fr]].
  destruct ((gap f x =? d) && free x [] (windows c)) eqn:B; [|left; exact E]. apply andb_true_iff in B as [_ B].
  destruct best as [b|]; [destruct (better _ _ _)|];
    [injection E as <-; right; split; [left; reflexivity|exact B]|left; exact E|injection E as <-; right; split; [left; reflexivity|exact B]].
Qed.
Lemma search_ok e c f w h d n r : search e c f w h d n = Some r -> free r [] (windows c) = true /\ rw r = w /\ rh r = h.
Proof.
  revert d. induction n as [|n IH]; simpl; intros d H; [discriminate|].
  destruct (pick e c f d None (cands f w h d)) as [q|] eqn:P; [|exact (IH _ H)]. injection H as <-.
  destruct (pick_ok _ _ _ _ _ _ _ P) as [E|[I Fr]]; [discriminate|]. split; [exact Fr|].
  unfold cands in I. apply in_flat_map in I as (i & _ & I). apply in_map_iff in I as (j & <- & _). split; reflexivity.
Qed.
Lemma spot_ok e c f w h : free (spot e c f w h) [] (windows c) = true /\ rw (spot e c f w h) = w /\ rh (spot e c f w h) = h.
Proof.
  unfold spot. cbv zeta. destruct (search _ _ _ _ _ _ _) as [r|] eqn:Fs; [exact (search_ok _ _ _ _ _ _ _ _ Fs)|].
  split; [|auto]. apply forallb_forall. intros o Ho. simpl. apply negb_true_iff, not_true_iff_false. rewrite overlaps_iff.
  assert (M : forall l, In o l -> right (wrect o) <= fold_right (fun o m => Z.max (right (wrect o)) m) (right f) l).
  { induction l as [|x l IH]; simpl; [contradiction|]. intros [<-|H]; [lia|specialize (IH H); lia]. }
  specialize (M (windows c) Ho). unfold right in *. simpl. lia.
Qed.
Lemma open_span_need e s m w : fst (need e m) <= fst (open_span e s m w) /\ snd (need e m) <= snd (open_span e s m w).
Proof. unfold open_span. destruct (need e m), w as [[]|]; simpl; lia. Qed.
Lemma add_shape e c i m w c' : add e c i m w = Some c' -> ~ In i (ids (windows c)) /\ zoom c' = zoom c /\
  focus c' = Some i /\ recent c' = i :: forget i (recent c) /\
  exists r, windows c' = windows c ++ [Window i r m] /\ (rw r, rh r) = open_span e (screen c) m w /\
    (focus_rect c <> None -> free r [] (windows c) = true).
Proof.
  unfold add. destruct (mem i (ids (windows c))) eqn:Mi; [discriminate|].
  destruct (open_span e (screen c) m w) as [ow oh] eqn:Os. intros H. injection H as <-. efv. simpl.
  split; [intros Hi; apply mem_iff in Hi; congruence|]. do 3 (split; [reflexivity|]). eexists. split; [reflexivity|].
  destruct (focus_rect c) as [f|]; [|split; [reflexivity|congruence]].
  destruct (spot_ok e c f ow oh) as (Fr & -> & ->). auto.
Qed.

(* check()'s focus clause: with windows on the canvas, the focus is one of them. *)
Definition focus_held c := focus_rect c = None -> windows c = [].
(* add and add_at append a window with a new id at open_span: on a free spot that keeps inv. *)
Lemma snoc_inv e c c' i m r s : inv e c -> ~ In i (ids (windows c)) -> windows c' = windows c ++ [Window i r m] ->
  (rw r, rh r) = open_span e (screen c) m s -> mins_ok e (windows c') /\ (free r [] (windows c) = true -> inv e c').
Proof.
  intros (Hn & Hd & Hm) Ni Ew Hr. unfold inv. rewrite Ew. assert (M : mins_ok e (windows c ++ [Window i r m])).
  { intros x Hx. apply in_app_or in Hx as [Hx|[<-|[]]]; [apply Hm; exact Hx|]. simpl.
    pose proof (open_span_need e (screen c) m s) as N. rewrite <- Hr in N. exact N. }
  split; [exact M|intros Fr; split; [unfold ids; rewrite map_app; apply nodup_snoc; auto|split; [|exact M]]].
  apply disjoint_snoc; [exact Hd|]. intros x Hx. unfold free in Fr. rewrite forallb_forall in Fr.
  specialize (Fr x Hx). simpl in Fr. apply negb_true_iff in Fr. exact Fr.
Qed.
Lemma add_inv e c i m w c' : inv e c -> add e c i m w = Some c' -> (focus_held c -> inv e c') /\ mins_ok e (windows c').
Proof.
  intros Hi H. destruct (add_shape _ _ _ _ _ _ H) as (Ni & _ & _ & _ & r & Ew & Hr & Fr).
  destruct (snoc_inv _ _ _ _ _ _ _ Hi Ni Ew Hr) as [M I]. split; [intros Hf; apply I|exact M].
  destruct (focus_rect c) eqn:F; [apply Fr; congruence|rewrite (Hf F); reflexivity].
Qed.

(* add_at appends one window at open_span on spot_near's spot, which is free, and moves no other. *)
Lemma in_sorted x l : In x (fold_right ins [] l) <-> In x l.
Proof.
  induction l as [|y l IH]; simpl; [tauto|]. rewrite <- IH. generalize (fold_right ins [] l) as k.
  induction k as [|q k IHk]; simpl; [tauto|]. destruct (nearer q y); simpl; rewrite ?IHk; tauto.
Qed.
Lemma far_spot ws a w h : free (Rect (ri a + far_past ws a) (rj a) w h) [] ws = true /\
  In (Rect (ri a + far_past ws a) (rj a) w h) (near ws a w h).
Proof.
  assert (F : 0 <= far_past ws a /\ forall o, In o ws -> right (wrect o) <= ri a + far_past ws a).
  { unfold far_past. split; [induction ws; simpl; lia|intros o Ho; pose proof (fold_max_ge ws (ri a) o Ho); lia]. }
  destruct F as [F0 Fo]. split.
  - apply forallb_forall. intros o Ho. simpl. apply negb_true_iff, not_true_iff_false. rewrite overlaps_iff.
    specialize (Fo o Ho). unfold right in Fo. simpl. lia.
  - unfold near. apply in_map_iff. exists (far_past ws a, 0). split; [simpl; f_equal; lia|].
    apply in_flat_map. exists (Z.to_nat (far_past ws a)). split; [apply in_seq; lia|]. unfold ring. cbv zeta. rewrite in_sorted.
    destruct (Z.to_nat (far_past ws a)) as [|n] eqn:N; [left; f_equal; lia|].
    apply in_or_app; right. apply in_flat_map. exists 0. split; [|right; left; f_equal; lia].
    unfold range. apply in_map_iff. exists n. split; [lia|apply in_seq; lia].
Qed.
(* C45: spot_near's search ends on a free spot at the span asked for: unwrap_or's fallback is never taken. *)
Theorem spot_near_ends_free ws a w h : find (fun r => free r [] ws) (near ws a w h) = Some (spot_near ws a w h) /\
  free (spot_near ws a w h) [] ws = true /\ rw (spot_near ws a w h) = w /\ rh (spot_near ws a w h) = h.
Proof.
  unfold spot_near. destruct (find _ _) as [r|] eqn:F.
  - apply find_some in F as [F G]. unfold near in F. apply in_map_iff in F as (p & <- & _). repeat split; auto.
  - destruct (far_spot ws a w h) as [Fr I]. rewrite (find_none _ _ F _ I) in Fr. discriminate.
Qed.
Lemma spot_near_at ws a w h : free (Rect (ri a) (rj a) w h) [] ws = true -> spot_near ws a w h = Rect (ri a) (rj a) w h.
Proof. intros Hf. unfold spot_near, near. simpl. rewrite !Z.add_0_r, Hf. reflexivity. Qed.
Lemma add_at_shape e c i m a c' : add_at e c i m a = Some c' -> ~ In i (ids (windows c)) /\ zoom c' = zoom c /\
  focus c' = Some i /\ recent c' = i :: forget i (recent c) /\
  exists r, windows c' = windows c ++ [Window i r m] /\ (rw r, rh r) = open_span e (screen c) m (Some (rw a, rh a)) /\
    free r [] (windows c) = true /\ (free (Rect (ri a) (rj a) (rw r) (rh r)) [] (windows c) = true -> r = Rect (ri a) (rj a) (rw r) (rh r)).
Proof.
  unfold add_at. destruct (mem i (ids (windows c))) eqn:Mi; [discriminate|].
  destruct (open_span e (screen c) m (Some (rw a, rh a))) as [ow oh] eqn:Os. intros H. injection H as <-. efv. simpl.
  split; [intros Hi; apply mem_iff in Hi; congruence|]. do 3 (split; [reflexivity|]). eexists. split; [reflexivity|].
  destruct (spot_near_ends_free (windows c) a ow oh) as (_ & Fr & -> & ->). split; [reflexivity|split; [exact Fr|apply spot_near_at]].
Qed.
(* C43, C44: add_at moves no other window, and the new one opens at open_span of the saved span,
   on the saved tile when that spot is free. *)
Theorem add_at_moves_no_window e c i m a c' : add_at e c i m a = Some c' -> exists r, windows c' = windows c ++ [Window i r m].
Proof. intros H. destruct (add_at_shape _ _ _ _ _ _ H) as (_ & _ & _ & _ & r & E & _). eauto. Qed.
Theorem add_at_lands_on_at e c i m a c' : add_at e c i m a = Some c' -> exists r, windows c' = windows c ++ [Window i r m] /\
  (rw r, rh r) = open_span e (screen c) m (Some (rw a, rh a)) /\
  (free (Rect (ri a) (rj a) (rw r) (rh r)) [] (windows c) = true -> r = Rect (ri a) (rj a) (rw r) (rh r)).
Proof. intros H. destruct (add_at_shape _ _ _ _ _ _ H) as (_ & _ & _ & _ & r & E & S & _ & L). eauto. Qed.

(* remove: the other windows close the lake; closing the focus focuses the head of recent. *)
Lemma remove_shape e c i g freed c' : rect_of i (windows c) = Some freed -> remove e c i g = Some c' ->
  let ws := closing (windows c) i freed g in
  let next := match forget i (recent c) with r :: _ => Some r | [] => nearest freed ws end in
  windows c' = ws /\ zoom c' = zoom c /\
  (focus c <> Some i -> focus c' = focus c /\ recent c' = forget i (recent c)) /\
  (focus c = Some i -> focus c' = next /\
     recent c' = match next with Some n => n :: forget n (forget i (recent c)) | None => forget i (recent c) end).
Proof.
  intros Hr H. unfold remove in H. rewrite Hr in H. cbv zeta in H. injection H as <-. cbv zeta. efv.
  destruct (focus c) as [f|]; [destruct (Nat.eqb f i) eqn:E; bools; [subst f|]|];
    [destruct (match forget i (recent c) with _ :: _ => _ | [] => _ end) | |]; simpl; repeat split; congruence.
Qed.

Lemma remove_inv e c i g c' : inv e c -> remove e c i g = Some c' -> inv e c'.
Proof.
  intros (Hn & Hd & Hm) H. destruct (rect_of i (windows c)) as [freed|] eqn:Hr;
    [|unfold remove in H; rewrite Hr in H; injection H as <-; split; auto].
  unfold inv. destruct (remove_shape _ _ _ _ _ _ Hr H) as (-> & _). destruct (without_ok i _ Hn) as (N1 & _ & N3).
  destruct (closing_same (windows c) i freed g) as [A D].
  split; [rewrite <- (same_ids _ _ A); exact N1|split; [apply D; intros a b Ha Hb; exact (Hd a b (N3 a Ha) (N3 b Hb))|]].
  apply (same_mins _ _ _ A). intros x Hx. exact (Hm x (N3 x Hx)).
Qed.

(* move_by shifts the window one tile, then pushes along d. *)
Lemma move_by_push_inv e c i d c' : good e -> inv e c -> move_by_push e c i d = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold move_by_push in H.
  destruct (window_of i (windows c)) as [w|] eqn:Hw; [|injection H as <-; split; auto].
  destruct (push _ _ _) as [ws|] eqn:Hp; [|discriminate]. injection H as <-. apply inv_efv.
  apply window_of_some in Hw as [Hwin Hwid]. destruct (Hm w Hwin). destruct (mins_spans _ _ Hg Hm w Hwin).
  destruct (regrow d i (shifted (wrect w) (fst (delta d)) (snd (delta d))) _ _ Hn (mins_spans _ _ Hg Hm) ltac:(rewrite <- Hwid; apply in_map, Hwin) ltac:(simpl; lia)
              ltac:(simpl; lia) Hp) as (Hn' & _ & Hd' & A).
  split; [exact Hn'|split; [exact Hd'|]]. refine (regrown_mins _ _ _ _ _ _ Hn Hwin Hwid Hm _ _ A); simpl; lia.
Qed.

Lemma move_by_inv e c i d p c' : good e -> inv e c -> move_by e c i d p = Some c' -> inv e c'.
Proof.
  intros Hg Hi H. unfold move_by in H. destruct (window_of i (windows c)) as [w|] eqn:Hw; [|injection H as <-; exact Hi].
  destruct (placed_by _ _ p) as [ws|] eqn:Pl; [|exact (move_by_push_inv _ _ _ _ _ Hg Hi H)].
  injection H as <-. apply inv_efv. destruct Hi as (Hn & _ & Hm).
  exact (placed_same_inv _ _ _ _ _ Pl Hn Hm (one_target _ _ _ (shifted (wrect w) (fst (delta d)) (snd (delta d))) Hn Hw eq_refl eq_refl)).
Qed.
Lemma resize_inv e c i ed d p c' : good e -> inv e c -> option_map snd (resize e c i ed d p) = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. destruct (resize e c i ed d p) as [[[|] c'']|] eqn:R; simpl in H; [| |discriminate];
    injection H as <-.
  2: { unfold resize in R. destruct (window_of i (windows c)); [destruct (refused _ _ _ _ _)|];
       cbv zeta in R; try (injection R as <-; split; auto).
       destruct (0 <? d); [destruct (placed_by _ _ _); [|destruct (push _ _ _)]|destruct (strip _ _ _ _)]; discriminate. }
  destruct (resize_cases _ _ _ _ _ _ _ R) as (w & Hw & Rf & _ & _ & Br). pose proof (window_of_some _ _ _ Hw) as [Hwin Hwid].
  destruct (refused_false _ _ _ _ _ Rf) as (Fw & Fh & _). set (new := grown ed (wrect w) d) in *.
  assert (Hs := mins_spans _ _ Hg Hm). destruct Hg as (_ & _ & Gn). destruct (Gn (wmin w)).
  unfold inv. destruct Br as [[Dp [Pl|Hp]] | [Dn (freed & side & St & ->)]].
  - destruct (placed_layouts_keep_the_rules _ _ _ _ Pl) as (A & D' & _).
    split; [rewrite <- (f2_ids _ _ _ (fun x y H => proj1 H) A); exact Hn|split; [exact D'|]].
    intros y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (E1 & E2 & E3)). rewrite <- E2.
    destruct (target_of (wid x) [(i, new)]) as [t|] eqn:T.
    + apply target_in in T as [T|[]]. injection T as T1 T2. rewrite E3, <- T2.
      assert (x = w) as -> by (apply (nodup_unique (windows c)); auto; congruence). split; assumption.
    + destruct E3 as [E3 E4]. rewrite E3, E4. exact (Hm x Hx).
  - destruct (regrow (outward ed) i new _ _ Hn Hs ltac:(rewrite <- Hwid; apply in_map, Hwin) ltac:(lia) ltac:(lia) Hp) as (Hn' & _ & Hd' & A).
    split; [exact Hn'|split; [exact Hd'|]]. exact (regrown_mins _ _ _ _ _ _ Hn Hwin Hwid Hm Fw Fh A).
  - assert (U : NoDup (ids (update i (set_rect new) (windows c))) /\ disjoint (update i (set_rect new) (windows c)) /\
                mins_ok e (update i (set_rect new) (windows c))).
    { split; [rewrite update_ids; auto|split].
      + apply (update_within_disjoint _ _ _ w Hn Hd Hwin Hwid). apply grown_within; exact Dn.
      + exact (regrown_mins _ _ _ _ _ _ Hn Hwin Hwid Hm Fw Fh (update_regrown _ _ _)). }
    destruct (splits _ _ _); [exact U|]. destruct U as (U1 & U2 & U3).
    pose proof (fill_same freed side (update i (set_rect new) (windows c))) as A.
    split; [rewrite <- (same_ids _ _ A); exact U1|split; [apply fill_disjoint, U2|exact (same_mins _ _ _ A U3)]].
Qed.

Lemma swap_push_inv e c n d c' : good e -> inv e c -> swap_push e c n d = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold swap_push in H.
  destruct (focus c) as [f|]; [|injection H as <-; split; auto].
  destruct (Nat.eqb f n); [injection H as <-; split; auto|].
  destruct (rect_of f (windows c)) as [ra|] eqn:Ra; [|injection H as <-; split; auto].
  destruct (window_of n (windows c)) as [wn|] eqn:Wn; [|injection H as <-; split; auto].
  destruct (swapped d ra (wrect wn)) as [fa nb] eqn:Sw.
  destruct (push d f _) as [ws1|] eqn:P1; [|discriminate].
  destruct (push (opposite d) n _) as [ws2|] eqn:P2; [|discriminate].
  injection H as <-. apply inv_efv. unfold inv; simpl.
  destruct (swap_ok _ _ _ _ _ _ _ _ _ _ Hn (mins_spans _ _ Hg Hm) Ra Wn Sw P1 P2) as (Hn2 & Hd2 & A & I).
  split; [exact Hn2|split; [exact Hd2|]]. apply (same_mins _ _ _ A). intros y Hy. exact (Hm y (I y Hy)).
Qed.

Lemma swap_targets ws f n ra wn d fa nb : NoDup (ids ws) -> rect_of f ws = Some ra -> window_of n ws = Some wn ->
  swapped d ra (wrect wn) = (fa, nb) ->
  forall x u, In x ws -> In (wid x, u) [(f, fa); (n, nb)] -> rw u = rw (wrect x) /\ rh u = rh (wrect x).
Proof.
  intros Hn Ra Wn Sw x u Hx Hu. destruct (swapped_spans _ _ _ _ _ Sw) as (S1 & S2 & S3 & S4).
  destruct Hu as [E|[E|[]]]; injection E as E1 E2; subst u.
  - destruct (rect_of_some _ _ _ Ra) as (y & Hy & Ey & Ry). assert (x = y) as -> by (apply (nodup_unique ws); auto; congruence).
    rewrite Ry. split; assumption.
  - apply window_of_some in Wn as [Wn Wi]. assert (x = wn) as -> by (apply (nodup_unique ws); auto; congruence). split; assumption.
Qed.
Lemma swap_inv e c n d p c' : good e -> inv e c -> swap e c n d p = Some c' -> inv e c'.
Proof.
  intros Hg Hi H. unfold swap in H. destruct (focus c) as [f|]; [|injection H as <-; exact Hi].
  destruct (Nat.eqb f n); [injection H as <-; exact Hi|].
  destruct (rect_of f (windows c)) as [ra|] eqn:Ra; [|injection H as <-; exact Hi].
  destruct (window_of n (windows c)) as [wn|] eqn:Wn; [|injection H as <-; exact Hi].
  destruct (swapped d ra (wrect wn)) as [fa nb] eqn:Sw.
  destruct (placed_by _ _ p) as [ws|] eqn:Pl; [|exact (swap_push_inv _ _ _ _ _ Hg Hi H)].
  injection H as <-. apply inv_efv. destruct Hi as (Hn & _ & Hm).
  exact (placed_same_inv _ _ _ _ _ Pl Hn Hm (swap_targets _ _ _ _ _ _ _ _ Hn Ra Wn Sw)).
Qed.
Lemma send_push_inv e c d c' : good e -> inv e c -> send_push e c d = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold send_push in H.
  destruct (focus c) as [f|]; [|injection H as <-; split; auto].
  destruct (_ || _) eqn:G; [injection H as <-; split; auto|]. bools.
  destruct (take f (windows c)) as [[w rest]|] eqn:T; [|discriminate].
  destruct (take_ok e _ _ _ _ Hn Hd Hm T) as (Hw & N1 & N2 & D1 & M1).
  apply window_of_some in Hw as [Hwin Hwid].
  destruct rest as [|o rest']; [discriminate|]. set (rest := o :: rest') in *. cbv zeta in H.
  injection H as <-. apply inv_efv. unfold inv. cbn [windows zoom with_windows].
  repeat match goal with |- context [?a :: ?b ++ [?x]] => change (a :: b ++ [x]) with ((a :: b) ++ [x]) end.
  split; [|split].
  - unfold ids; rewrite map_app. apply nodup_snoc; [exact N1|]. simpl. rewrite Hwid. exact N2.
  - apply disjoint_snoc; [exact D1|]. intros x Hx. apply not_true_iff_false. rewrite overlaps_iff. simpl.
    destruct d; simpl in *; try discriminate.
    + pose proof (fold_min_le rest (ri (wrect o)) x Hx) as P. simpl in P. lia.
    + pose proof (fold_max_ge rest (right (wrect o)) x Hx) as P. simpl in P. unfold right in *. lia.
  - intros x Hx. apply in_app_or in Hx as [Hx|[<-|[]]]; [apply M1; exact Hx|]. simpl. apply Hm; exact Hwin.
Qed.

Lemma send_inv e c d p c' : good e -> inv e c -> send_to_edge e c d p = Some c' -> inv e c'.
Proof.
  intros Hg Hi H. unfold send_to_edge in H. destruct (focus c) as [f|]; [|injection H as <-; exact Hi].
  destruct (_ || _); [injection H as <-; exact Hi|].
  destruct (window_of f (windows c)) as [w|] eqn:Hw; [|exact (send_push_inv _ _ _ _ Hg Hi H)].
  destruct (without f (windows c)) as [|o rest]; [exact (send_push_inv _ _ _ _ Hg Hi H)|].
  destruct (placed_by _ _ p) as [ws|] eqn:Pl; [|exact (send_push_inv _ _ _ _ Hg Hi H)].
  injection H as <-. apply inv_efv. destruct Hi as (Hn & _ & Hm).
  exact (placed_same_inv _ _ _ _ _ Pl Hn Hm (one_target _ _ _ (Rect (edge_i d w o rest) (rj (wrect w)) (rw (wrect w)) (rh (wrect w))) Hn Hw eq_refl eq_refl)).
Qed.
Lemma moved_to_inv e i x y ws ws' : good e -> NoDup (ids ws) -> disjoint ws -> mins_ok e ws -> moved_to i x y ws = Some ws' ->
  NoDup (ids ws') /\ disjoint ws' /\ mins_ok e ws'.
Proof.
  intros Hg Hn Hd Hm H. unfold moved_to in H.
  destruct (take i ws) as [[w rest]|] eqn:T; [|injection H as <-; auto].
  destruct (take_ok e _ _ _ _ Hn Hd Hm T) as (Hw & N1 & N2 & D1 & M1).
  apply window_of_some in Hw as [Hwin Hwid]. destruct (Hm w Hwin).
  rewrite <- Hwid in H, N2. destruct Hg as (G1 & G2 & Gn). destruct (Gn (wmin w)).
  destruct (append_push Right rest (set_rect (Rect x y (rw (wrect w)) (rh (wrect w))) w) ws' N1 N2
              (mins_spans _ _ (conj G1 (conj G2 Gn)) M1) ltac:(simpl; lia) ltac:(simpl; lia) H) as (Hn' & _ & Hd' & A).
  split; [exact Hn'|split; [exact Hd'|]]. apply (same_mins _ _ _ A).
  intros u Hu. apply in_app_or in Hu as [Hu|[<-|[]]]; [apply M1; exact Hu|]. simpl. auto.
Qed.
Lemma move_inv e c i x y p g c' : good e -> inv e c -> move_to e c i x y p g = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold move_to in H.
  destruct (rect_of i (windows c)) as [r|] eqn:Hr; [|injection H as <-; split; auto].
  destruct (rect_eqb _ r); [injection H as <-; split; auto|].
  assert (K : forall u, NoDup (ids u) /\ disjoint u /\ mins_ok e u ->
    inv e (ensure_focus_visible e (with_windows c (if is_bridge (windows c) i then gathered u g else u)))).
  { intros u (N1 & D1 & M1). apply inv_efv. unfold inv; simpl. destruct (is_bridge _ _); [apply gathered_inv; auto|auto]. }
  destruct (placed_by _ _ p) as [u|] eqn:Pl.
  - injection H as <-. apply K. destruct (rect_of_some _ _ _ Hr) as (w & Hwin & Hwid & Hwr).
    exact (placed_same_inv _ _ _ _ _ Pl Hn Hm (one_target _ _ _ (Rect x y (rw r) (rh r)) Hn (window_of_in _ _ _ Hn Hwin Hwid)
      ltac:(rewrite Hwr; reflexivity) ltac:(rewrite Hwr; reflexivity))).
  - destruct (moved_to i x y (windows c)) as [u|] eqn:Mt; [|discriminate]. injection H as <-. apply K.
    exact (moved_to_inv _ _ _ _ _ _ Hg Hn Hd Hm Mt).
Qed.

Lemma enforce_all_inv e a ws ws' : good e -> NoDup (ids ws) -> spans_ok ws -> disjoint ws -> enforce_all e a ws = Some ws' ->
  NoDup (ids ws') /\ disjoint ws' /\ mins_ok e ws'.
Proof.
  intros Hg Hn Hs Hd Ha. destruct (enforce_all_ok _ _ _ _ Hg Hn Hs Hd Ha) as (Hn' & _ & Hd' & A).
  split; [exact Hn'|split; [exact Hd'|]].
  intros y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (x & Hx & (E1 & E2 & E3 & E4)).
  rewrite <- E2, E3, E4, (proj2 (mem_iff (wid x) (ids ws)) (in_map wid _ _ Hx)). lia.
Qed.

(* C16: the zoom is a camera. zoom_by keeps every window, the focus and recent, sets the clamped
   zoom, and changes nothing when that zoom is the current one. *)
Lemma restore_inv e c l a c' : good e -> inv e c -> restore e c l a = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold restore in H. destruct (restored (windows c) l) as [ws|] eqn:R; [|injection H as <-; split; auto].
  destruct (enforce_all e a ws) as [ws'|] eqn:Ea; [|discriminate]. injection H as <-. apply inv_efv.
  destruct (restored_ok _ _ _ R) as (A & S & D).
  exact (enforce_all_inv _ _ _ _ Hg ltac:(rewrite <- (f2_ids _ _ _ (fun x y H => proj1 H) A); exact Hn) S D Ea).
Qed.
Theorem zoom_is_a_camera e c d :
  windows (zoom_by e c d) = windows c /\ focus (zoom_by e c d) = focus c /\ recent (zoom_by e c d) = recent c /\
  zoom (zoom_by e c d) = Z.max (zoom_min e) (Z.min (zoom_max e) (zoom c + d)) /\ (zoom (zoom_by e c d) = zoom c -> zoom_by e c d = c).
Proof.
  unfold zoom_by. destruct (_ =? zoom c) eqn:E; bools; [repeat split; auto|efv; simpl; repeat split; auto; lia].
Qed.

Lemma set_config_inv e e' a c c' : good e -> good e' -> inv e c -> set_config e' a c = Some c' -> inv e' c'.
Proof.
  intros Hg Hg' (Hn & Hd & Hm) H. unfold set_config in H. cbv zeta in H.
  destruct (enforce_all e' a (windows c)) as [ws|] eqn:Ha; [|discriminate]. injection H as <-. apply inv_efv.
  exact (enforce_all_inv _ _ _ _ Hg' Hn (mins_spans _ _ Hg Hm) Hd Ha).
Qed.

(* rename relabels one window: ids stay unique because the new one is not taken. *)
Lemma rename_inv e c i j : inv e c -> inv e (rename c i j).
Proof.
  intros (Hn & Hd & Hm). unfold rename. destruct (mem j (ids (windows c))) eqn:Ej; [exact (conj Hn (conj Hd Hm))|].
  destruct (window_of i (windows c)) as [w0|]; [|exact (conj Hn (conj Hd Hm))].
  assert (Hj : ~ In j (ids (windows c))) by (rewrite <- mem_iff; congruence). unfold inv, update; simpl.
  split; [|split].
  - clear Hd Hm Ej w0. revert Hn Hj. unfold ids. rewrite map_map. induction (windows c) as [|w ws IH]; simpl; intros Hn Hj; [constructor|].
    inversion Hn as [|? ? Hw Hn']; subst. constructor; [|apply IH; auto].
    rewrite in_map_iff. intros (x & E & Hx). apply Hw.
    destruct (Nat.eqb (wid w) i) eqn:A, (Nat.eqb (wid x) i) eqn:B; simpl in E; bools;
      [rewrite A, <- B | exfalso; apply Hj; right; rewrite <- E | exfalso; apply Hj; left; congruence | rewrite <- E];
      apply in_map; auto.
  - intros a' b' Ha Hb Hab. apply in_map_iff in Ha as (a & <- & Ha). apply in_map_iff in Hb as (b & <- & Hb).
    assert (E : wid a <> wid b) by (intros E; rewrite (nodup_unique _ _ _ Hn Ha Hb E) in Hab; contradiction).
    destruct (Nat.eqb (wid a) i), (Nat.eqb (wid b) i); exact (Hd a b Ha Hb E).
  - intros y Hy. apply in_map_iff in Hy as (x & <- & Hx). destruct (Nat.eqb (wid x) i); exact (Hm x Hx).
Qed.


Lemma set_min_inv e c i m a c' : good e -> inv e c -> set_min e c i m a = Some c' -> inv e c'.
Proof.
  intros Hg (Hn & Hd & Hm) H. unfold set_min in H.
  destruct (window_of i (windows c)) as [w|]; [|injection H as <-; split; auto].
  set (f := fun w : window => Window (wid w) (wrect w) m) in H.
  destruct (enforce_min e a i (update i f (windows c))) as [ws|] eqn:He; [|discriminate].
  injection H as <-. apply inv_efv. unfold inv; simpl.
  assert (Hs : spans_ok (update i f (windows c))).
  { intros x Hx. unfold update in Hx. apply in_map_iff in Hx as (y & <- & Hy). pose proof (mins_spans _ _ Hg Hm y Hy).
    destruct (Nat.eqb (wid y) i); simpl; auto. }
  destruct (enforce_min_ok e a i (update i f (windows c)) ws Hg ltac:(rewrite update_ids; auto) Hs
              (update_rects_disjoint i f _ (fun w => conj eq_refl eq_refl) Hd) He) as (Hn' & _ & Hd' & A).
  split; [exact Hn'|split; [exact Hd'|]].
  intros y Hy. apply (grew_mins _ _ (fun _ => True) _ _ A); auto.
  intros x Hx Hxi _. unfold update in Hx. apply in_map_iff in Hx as (u & <- & Hu).
  destruct (Nat.eqb (wid u) i) eqn:E; bools; [simpl in Hxi; contradiction|]. apply Hm; exact Hu.
Qed.

Theorem step_keeps_inv e c o c' : good e -> good (env_after e o) -> inv e c -> (forall i m w, o = Add i m w -> focus_held c) ->
  step e c o = Some c' -> inv (env_after e o) c'.
Proof.
  intros Hg Hg' Hi Hf H. destruct o; simpl in H; simpl env_after.
  - exact (proj1 (add_inv _ _ _ _ _ _ Hi H) (Hf _ _ _ eq_refl)).
  - exact (move_by_inv _ _ _ _ _ _ Hg Hi H).
  - exact (remove_inv _ _ _ _ _ Hi H).
  - injection H as <-. unfold focus_id. destruct (mem _ _); [apply inv_efv|]; exact Hi.
  - exact (resize_inv _ _ _ _ _ _ _ Hg Hi H).
  - exact (swap_inv _ _ _ _ _ _ Hg Hi H).
  - exact (send_inv _ _ _ _ _ Hg Hi H).
  - exact (move_inv _ _ _ _ _ _ _ _ Hg Hi H).
  - injection H as <-. unfold inv. rewrite (proj1 (zoom_is_a_camera e c d)). exact Hi.
  - exact (set_min_inv _ _ _ _ _ _ Hg Hi H).
  - injection H as <-. apply inv_efv. exact Hi.
  - exact (set_config_inv _ _ _ _ _ Hg Hg' Hi H).
  - injection H as <-. exact (rename_inv _ _ _ _ Hi).
  - destruct (add_at_shape _ _ _ _ _ _ H) as (Ni & _ & _ & _ & r & Ew & Hr & Fr & _). exact (proj2 (snoc_inv _ _ _ _ _ _ _ Hi Ni Ew Hr) Fr).
  - exact (restore_inv _ _ _ _ _ Hg Hi H).
  - injection H as <-. unfold gather_islands. apply inv_efv. destruct Hi as (Hn & Hd & Hm). exact (gathered_inv _ _ _ Hn Hd Hm).
Qed.

(* C1 and C5: from a canvas that holds them, every operation keeps windows disjoint and at
   least at the span their minimum needs, which no zoom changes; add, which never pushes, needs
   check()'s focus clause to find a free spot (add_without_a_focus_overlaps). *)
Theorem no_overlap e c o c' : good e -> good (env_after e o) -> inv e c -> (forall i m w, o = Add i m w -> focus_held c) ->
  step e c o = Some c' -> disjoint (windows c').
Proof. intros Hg Hg' Hi Hf H. apply (step_keeps_inv _ _ _ _ Hg Hg' Hi Hf H). Qed.
Theorem minimum_spans e c o c' : good e -> good (env_after e o) -> inv e c -> step e c o = Some c' ->
  mins_ok (env_after e o) (windows c').
Proof.
  intros Hg Hg' Hi H. destruct o; try (apply (step_keeps_inv _ _ _ _ Hg Hg' Hi (fun _ _ _ E => ltac:(discriminate E)) H)).
  exact (proj2 (add_inv _ _ _ _ _ _ Hi H)).
Qed.
Example add_without_a_focus_overlaps : option_map (fun c => map wrect (windows c))
  (add shipped (Canvas [Window 1 (Rect 0 0 1 1) (0, 0)] None 0 (0, 0) (14400000, 9300000) [1%nat]) 2 (0, 0) None)
  = Some [Rect 0 0 1 1; Rect 0 0 1 1].
Proof. reflexivity. Qed.
(* The shape of every operation: unchanged, a rename, or ensure_focus_visible under the env it
   leaves of a canvas at the same zoom unless the operation is zoom_by or set_config. *)
Ltac crush H := repeat (cbv beta iota zeta in H; match type of H with
  | Some _ = Some _ => injection H as H
  | None = Some _ => discriminate H
  | context [match ?x with _ => _ end] =>
      lazymatch x with context [match _ with _ => _ end] => fail | _ => destruct x eqn:? end
  end).

Lemma step_frame e c o c' : step e c o = Some c' ->
  (c' = c /\ env_after e o = e) \/ (exists i j, o = Rename i j /\ c' = rename c i j) \/
  exists x, c' = ensure_focus_visible (env_after e o) x /\
    ((forall d, o <> ZoomBy d) -> (forall e' a, o <> SetConfig e' a) -> zoom x = zoom c).
Proof.
  intros H. destruct o; simpl in H;
    unfold add, add_at, move_by, move_by_push, remove, focus_id, option_map, resize, swap, swap_push, send_to_edge, send_push,
      move_to, zoom_by, set_min, set_screen, set_config, restore, gather_islands in H; crush H; subst;
    first [left; split; reflexivity | right; left; do 2 eexists; split; reflexivity
          | right; right; eexists; split; [reflexivity|intros Hz Hs;
              first [reflexivity | exfalso; eapply Hz; reflexivity | exfalso; eapply Hs; reflexivity]]].
Qed.

(* C2: every operation but zoom_by and set_config keeps the zoom. *)
Theorem only_zoom_by_zooms e c o c' : (forall d, o <> ZoomBy d) -> (forall e' a, o <> SetConfig e' a) ->
  step e c o = Some c' -> zoom c' = zoom c.
Proof.
  intros Ho Hs H. destruct (step_frame _ _ _ _ H) as [[-> _]|[(i & j & _ & ->)|(x & -> & Hx)]];
    [reflexivity|unfold rename; destruct (mem _ _); [|destruct (window_of _ _)]; reflexivity|].
  rewrite (proj1 (proj2 (proj2 (efv_frame _ x)))). exact (Hx Ho Hs).
Qed.

(* set_config changes the zoom only by clamping it into the new bounds. *)
Theorem set_config_only_clamps e a c c' : set_config e a c = Some c' ->
  zoom c' = Z.max (zoom_min e) (Z.min (zoom_max e) (zoom c)) /\ (zoom_min e <= zoom c <= zoom_max e -> zoom c' = zoom c).
Proof.
  unfold set_config. intros H. destruct (enforce_all _ _ _); [|discriminate]. injection H as <-.
  rewrite (proj1 (proj2 (proj2 (efv_frame _ _)))). simpl. lia.
Qed.

(* recent holds each window id once with the focus first, as check() and the harness test after
   every operation; it gives check()'s focus clause, which add needs. *)
Definition recency_ok c := Permutation (recent c) (ids (windows c)) /\ focus c = hd_error (recent c).
Definition focus_known c := forall f, focus c = Some f -> In f (ids (windows c)).
Lemma in_ids_rect i ws : In i (ids ws) -> exists r, rect_of i ws = Some r.
Proof.
  intros H. unfold rect_of. destruct (window_of i ws) as [w|] eqn:E; [simpl; eauto|].
  apply in_map_iff in H as (w & Ew & Hw). exfalso. exact (window_of_none _ _ E w Hw Ew).
Qed.
Lemma recency_held c : recency_ok c -> focus_held c /\ focus_known c.
Proof.
  intros (P & F). assert (K : focus_known c).
  { intros f Hf. apply (Permutation_in _ P). rewrite F in Hf. destruct (recent c); simpl in Hf; [discriminate|].
    injection Hf as ->. left; reflexivity. }
  split; [|exact K]. intros Hr. destruct (focus c) as [f|] eqn:Ef.
  - destruct (in_ids_rect f _ (K f Ef)) as (r & Er). unfold focus_rect in Hr. rewrite Ef, Er in Hr. discriminate.
  - destruct (recent c) eqn:R; [|discriminate]. apply Permutation_nil in P. unfold ids in P.
    exact (map_eq_nil _ _ P).
Qed.

Lemma forget_cons i x l : forget i (x :: l) = if Nat.eqb x i then forget i l else x :: forget i l.
Proof. unfold forget; simpl. destruct (Nat.eqb x i); reflexivity. Qed.
Lemma forget_notin i l : ~ In i l -> forget i l = l.
Proof. induction l as [|x l IH]; [reflexivity|]. rewrite forget_cons. intros H. destruct (Nat.eqb x i) eqn:E; bools; simpl in H; [tauto|f_equal; auto]. Qed.
Lemma forget_perm n l : NoDup l -> In n l -> Permutation (n :: forget n l) l.
Proof.
  induction l as [|x l IH]; intros Hn Hi; [contradiction|]. inversion Hn as [|? ? Hx Hl]; subst. rewrite forget_cons.
  destruct (Nat.eqb x n) eqn:E; bools; [subst; rewrite forget_notin; auto|].
  destruct Hi as [->|Hi]; [congruence|]. eapply perm_trans; [apply perm_swap|]. apply perm_skip, IH; auto.
Qed.
Lemma forget_same i l l' : Permutation l l' -> Permutation (forget i l) (forget i l').
Proof.
  induction 1; rewrite ?forget_cons; [constructor| |destruct (Nat.eqb x i), (Nat.eqb y i); first [reflexivity|apply perm_swap]
    |eapply perm_trans; eassumption].
  destruct (Nat.eqb x i); [|constructor]; assumption.
Qed.
Lemma forget_head i l : hd_error (forget i l) = find (fun r => negb (Nat.eqb r i)) l.
Proof. induction l as [|x l IH]; [reflexivity|]. rewrite forget_cons. simpl. destruct (Nat.eqb x i); simpl; auto. Qed.
Lemma forget_ids i ws : ids (without i ws) = forget i (ids ws).
Proof.
  induction ws as [|w ws IH]; [reflexivity|]. change (ids (w :: ws)) with (wid w :: ids ws). rewrite forget_cons, <- IH.
  unfold without; simpl. destruct (Nat.eqb (wid w) i); reflexivity.
Qed.

(* Every operation but add, remove, focus and rename keeps recent, the focus and the window ids. *)
Lemma push_ids d m ws ws' : push d m ws = Some ws' -> ids ws' = ids ws.
Proof. intros H. symmetry. exact (f2_ids _ _ _ (fun x y H => proj1 H) (push_same_at _ _ _ _ H)). Qed.
Lemma fill_ids freed side ws : ids (fill freed side ws) = ids ws.
Proof. symmetry. apply same_ids, fill_same. Qed.
Lemma grow_to_ids ws m t d p ws' : grow_to ws m t d p = Some ws' -> ids ws' = ids ws.
Proof.
  unfold grow_to. destruct (placed_by _ _ p) eqn:Pl; [intros H; injection H as <-; exact (placed_ids _ _ _ _ Pl)|].
  intros H. rewrite (push_ids _ _ _ _ H). apply update_ids; reflexivity.
Qed.
Lemma enforce_min_ids e a m ws ws' : enforce_min e a m ws = Some ws' -> ids ws' = ids ws.
Proof.
  assert (G : forall g v v', (g = widen e a m \/ g = heighten e a m) -> g v = Some v' -> ids v' = ids v).
  { intros g v v' [->| ->] H; unfold widen, heighten in H; destruct (window_of m v); try destruct (_ <? _);
      try (injection H as <-; reflexivity); exact (grow_to_ids _ _ _ _ _ _ H). }
  unfold enforce_min. destruct (widen e a m ws) as [v|] eqn:W; [|discriminate]. intros H.
  rewrite (G _ _ _ (or_intror eq_refl) H). exact (G _ _ _ (or_introl eq_refl) W).
Qed.
Lemma enforce_all_ids e a ws ws' : enforce_all e a ws = Some ws' -> ids ws' = ids ws.
Proof.
  assert (G : forall l v, fold_left (fun acc i => match acc with Some ws => enforce_min e a i ws | None => None end) l (Some v)
    = Some ws' -> ids ws' = ids v).
  { induction l as [|i l IH]; simpl; intros v H; [congruence|].
    destruct (enforce_min e a i v) as [v1|] eqn:E; [rewrite (IH v1 H); exact (enforce_min_ids _ _ _ _ _ E)|].
    exfalso. clear IH. induction l; simpl in H; [discriminate|auto]. }
  exact (G _ _).
Qed.
Lemma take_ids i ws w rest : NoDup (ids ws) -> take i ws = Some (w, rest) -> Permutation (ids rest ++ [wid w]) (ids ws).
Proof.
  unfold take. intros Hn H. destruct (window_of i ws) as [w0|] eqn:Hw; [|discriminate]. injection H as <- <-.
  unfold closed. rewrite !fill_ids, forget_ids. apply window_of_some in Hw as [Hw <-].
  eapply perm_trans; [apply Permutation_sym, Permutation_cons_append|]. apply forget_perm; [exact Hn|apply in_map, Hw].
Qed.
Lemma swap_push_ids e c n d c' : NoDup (ids (windows c)) -> swap_push e c n d = Some c' ->
  recent c' = recent c /\ focus c' = focus c /\ Permutation (ids (windows c')) (ids (windows c)).
Proof.
  intros Hn H. unfold swap_push in H.
  destruct (focus c) as [f|] eqn:Ef; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (Nat.eqb f n); [injection H as <-; repeat split; first [reflexivity|exact Ef]|].
  destruct (rect_of f (windows c)) as [ra|]; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (window_of n (windows c)) as [wn|] eqn:Wn; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (swapped d ra (wrect wn)) as [fa nb].
  destruct (push d f _) as [ws1|] eqn:P1; [|discriminate].
  destruct (push (opposite d) n _) as [ws2|] eqn:P2; [|discriminate].
  injection H as <-. efv. cbn [recent focus windows with_windows]. split; [reflexivity|split; [exact Ef|]].
  rewrite (push_ids _ _ _ _ P2). unfold ids at 1. rewrite map_app. fold (ids ws1).
  rewrite (push_ids _ _ _ _ P1), (update_ids f (set_rect fa) _ (fun _ => eq_refl)), forget_ids.
  apply window_of_some in Wn as [Hwn <-]. simpl.
  eapply perm_trans; [apply Permutation_sym, Permutation_cons_append|]. apply forget_perm; [exact Hn|apply in_map, Hwn].
Qed.
Lemma swap_ids e c n d p c' : NoDup (ids (windows c)) -> swap e c n d p = Some c' ->
  recent c' = recent c /\ focus c' = focus c /\ Permutation (ids (windows c')) (ids (windows c)).
Proof.
  intros Hn H. unfold swap in H.
  destruct (focus c) as [f|] eqn:Ef; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (Nat.eqb f n); [injection H as <-; repeat split; first [reflexivity|exact Ef]|].
  destruct (rect_of f (windows c)) as [ra|]; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (window_of n (windows c)) as [wn|]; [|injection H as <-; repeat split; first [reflexivity|exact Ef]].
  destruct (swapped d ra (wrect wn)) as [fa nb]. destruct (placed_by _ _ p) as [ws|] eqn:Pl; [|rewrite <- Ef; exact (swap_push_ids _ _ _ _ _ Hn H)].
  injection H as <-. efv. cbn [recent focus windows with_windows]. rewrite (placed_ids _ _ _ _ Pl). split; [reflexivity|split; [exact Ef|reflexivity]].
Qed.
Lemma moved_to_ids i x y ws ws' : NoDup (ids ws) -> moved_to i x y ws = Some ws' -> Permutation (ids ws') (ids ws).
Proof.
  unfold moved_to. intros Hn H. destruct (take i ws) as [[w rest]|] eqn:T; [|injection H as <-; reflexivity].
  rewrite (push_ids _ _ _ _ H). eapply perm_trans; [|exact (take_ids _ _ _ _ Hn T)]. unfold ids; rewrite map_app. reflexivity.
Qed.
Lemma step_ids e c o c' : NoDup (ids (windows c)) -> step e c o = Some c' -> (forall i m a, o <> AddAt i m a) ->
  (forall i m w, o <> Add i m w) -> (forall i g, o <> Remove i g) -> (forall i, o <> FocusId i) -> (forall i j, o <> Rename i j) ->
  recent c' = recent c /\ focus c' = focus c /\ Permutation (ids (windows c')) (ids (windows c)).
Proof.
  intros Hn H B A R F N. destruct o; try (exfalso; first [eapply A; reflexivity | eapply R; reflexivity | eapply F; reflexivity | eapply N; reflexivity | eapply B; reflexivity]); simpl in H;
    try (lazymatch type of H with swap _ _ _ _ _ = _ => exact (swap_ids _ _ _ _ _ _ Hn H) end);
    unfold move_by, move_by_push, option_map, resize, send_to_edge, send_push, move_to, zoom_by, set_min, set_screen, set_config, restore,
      gather_islands in H;
    crush H; subst; efv; simpl; (split; [congruence|split; [congruence|]]); rewrite ?gathered_ids;
    repeat match goal with
    | P : push _ _ _ = Some _ |- _ => rewrite (push_ids _ _ _ _ P); clear P
    | P : placed_by _ _ _ = Some _ |- _ => rewrite (placed_ids _ _ _ _ P); clear P
    | P : enforce_min _ _ _ _ = Some _ |- _ => rewrite (enforce_min_ids _ _ _ _ _ P); clear P
    | P : enforce_all _ _ _ = Some _ |- _ => rewrite (enforce_all_ids _ _ _ _ P); clear P
    | P : restored _ _ = Some _ |- _ => rewrite (restored_ids _ _ _ P); clear P
    | P : moved_to _ _ _ _ = Some _ |- _ => exact (moved_to_ids _ _ _ _ _ Hn P)
    | P : take _ _ = Some (_, _) |- _ => eapply perm_trans; [|exact (take_ids _ _ _ _ Hn P)]; unfold ids; rewrite map_app; reflexivity
    end; rewrite ?fill_ids, ?update_ids; try reflexivity; intros; reflexivity.
Qed.

Lemma recency_efv e c : recency_ok c -> recency_ok (ensure_focus_visible e c).
Proof. intros [P F]. destruct (efv_frame e c) as (E1 & E2 & _ & _ & E5). split; [rewrite E1, E5|rewrite E2, E5]; assumption. Qed.
Lemma set_focus_ok c n : Permutation (recent c) (ids (windows c)) -> NoDup (recent c) -> In n (recent c) -> recency_ok (set_focus c n).
Proof. intros P N I. split; [simpl; eapply perm_trans; [apply forget_perm; auto|exact P]|reflexivity]. Qed.
Lemma none_left i freed g ws l : Permutation l (ids ws) -> forget i l = [] -> closing ws i freed g = [].
Proof.
  intros P E. pose proof (forget_same i _ _ P) as Q. rewrite E, <- forget_ids in Q. apply Permutation_nil in Q.
  apply (map_eq_nil wid). fold (ids (closing ws i freed g)). rewrite closing_ids. exact Q.
Qed.

Lemma snoc_recency c c' i m r : ~ In i (ids (windows c)) -> focus c' = Some i -> recent c' = i :: forget i (recent c) ->
  windows c' = windows c ++ [Window i r m] -> recency_ok c -> recency_ok c'.
Proof.
  intros Ni Ef Er Ew (P & F). rewrite forget_notin in Er by (intros X; apply Ni, (Permutation_in _ P X)).
  split; [rewrite Er, Ew; unfold ids; rewrite map_app; simpl; eapply perm_trans; [apply perm_skip, P|apply Permutation_cons_append]
         |rewrite Ef, Er; reflexivity].
Qed.
(* C42: add_at keeps check()'s invariants without its focus clause, which add needs, and focuses the new window. *)
Theorem add_at_keeps_inv e c i m a c' : add_at e c i m a = Some c' ->
  (inv e c -> inv e c') /\ (recency_ok c -> recency_ok c') /\ focus c' = Some i.
Proof.
  intros H. destruct (add_at_shape _ _ _ _ _ _ H) as (Ni & _ & Ef & Er & r & Ew & Hr & Fr & _).
  exact (conj (fun Hi => proj2 (snoc_inv _ _ _ _ _ _ _ Hi Ni Ew Hr) Fr) (conj (snoc_recency _ _ _ _ _ Ni Ef Er Ew) Ef)).
Qed.

(* C30: every operation keeps recent a permutation of the window ids with the focus at its head. *)
Theorem recency_kept e c o c' : NoDup (ids (windows c)) -> recency_ok c -> step e c o = Some c' -> recency_ok c'.
Proof.
  intros Hn (P & F) H. assert (Nr : NoDup (recent c)) by exact (Permutation_NoDup (Permutation_sym P) Hn).
  assert (Fr : (forall i m a, o <> AddAt i m a) -> (forall i m w, o <> Add i m w) -> (forall i g, o <> Remove i g) ->
    (forall i, o <> FocusId i) -> (forall i j, o <> Rename i j) -> recency_ok c').
  { intros B A R Fo N. destruct (step_ids _ _ _ _ Hn H B A R Fo N) as (E1 & E2 & E3).
    split; [rewrite E1; eapply perm_trans; [exact P|apply Permutation_sym, E3]|congruence]. }
  destruct o; try (apply Fr; intros; discriminate); simpl in H.
  - destruct (add_shape _ _ _ _ _ _ H) as (Ni & _ & Ef & Er & r & Ew & _). exact (snoc_recency _ _ _ _ _ Ni Ef Er Ew (conj P F)).
  - destruct (rect_of i (windows c)) as [freed|] eqn:Hr; [|unfold remove in H; rewrite Hr in H; injection H as <-; split; auto].
    destruct (remove_shape _ _ _ _ _ _ Hr H) as (Ew & _ & Fo & Fi). cbv zeta in *.
    assert (P' : Permutation (forget i (recent c)) (ids (windows c'))) by (rewrite Ew, closing_ids, forget_ids; apply forget_same, P).
    unfold recency_ok. destruct (focus c) as [f|] eqn:Ef; [destruct (Nat.eq_dec f i) as [->|Nf]|].
    + destruct (Fi eq_refl) as [F' R']. destruct (forget i (recent c)) as [|r l] eqn:G.
      * rewrite (none_left _ _ _ _ _ P G) in F', R'. simpl in F', R'. rewrite F', R'. split; [exact P'|reflexivity].
      * rewrite F', R'. split; [|reflexivity]. eapply perm_trans; [|exact P']. apply forget_perm; [|left; reflexivity].
        rewrite <- G. apply NoDup_filter, Nr.
    + destruct (Fo ltac:(congruence)) as [F' R']. rewrite F', R'. split; [exact P'|]. rewrite forget_head, F.
      destruct (recent c) as [|x l]; simpl in F |- *; [discriminate|]. injection F as ->. apply Nat.eqb_neq in Nf. rewrite Nf. reflexivity.
    + destruct (Fo ltac:(congruence)) as [F' R']. rewrite F', R'. split; [exact P'|]. rewrite F.
      destruct (recent c); [reflexivity|discriminate].
  - injection H as <-. unfold focus_id. destruct (mem i (ids (windows c))) eqn:M; [|split; auto].
    apply recency_efv, set_focus_ok; auto. apply (Permutation_in _ (Permutation_sym P)), mem_iff, M.
  - injection H as <-. unfold rename. destruct (mem j (ids (windows c))); [split; auto|].
    destruct (window_of i (windows c)); [|split; auto]. split; simpl.
    + replace (ids (update i _ (windows c))) with (map (fun r => if Nat.eqb r i then j else r) (ids (windows c))).
      * apply Permutation_map, P.
      * unfold ids, update. rewrite !map_map. apply map_ext. intros x. destruct (Nat.eqb (wid x) i); reflexivity.
    + rewrite F. destruct (recent c); reflexivity.
  - exact (proj1 (proj2 (add_at_keeps_inv _ _ _ _ _ _ H)) (conj P F)).
Qed.

(* C31: closing the focused window focuses the first id in recent other than it. *)
Theorem closing_the_focus_focuses_the_last_used e c i g c' : NoDup (ids (windows c)) -> recency_ok c -> focus c = Some i ->
  remove e c i g = Some c' -> focus c' = find (fun r => negb (Nat.eqb r i)) (recent c).
Proof.
  intros Hn (P & F) Fi H. destruct (in_ids_rect i (windows c) (proj2 (recency_held c (conj P F)) i Fi)) as (freed & Hr).
  destruct (remove_shape _ _ _ _ _ _ Hr H) as (_ & _ & _ & Hf). destruct (Hf Fi) as [-> _].
  rewrite <- forget_head. destruct (forget i (recent c)) as [|r l] eqn:G; [|reflexivity].
  rewrite (none_left _ _ _ _ _ P G). reflexivity.
Qed.

Theorem every_trace_keeps_inv e os s c' : good e -> (forall e' a, In (SetConfig e' a) os -> good e') ->
  run e (Canvas [] None 0 (0, 0) s []) os = Some c' -> inv (fold_left env_after os e) c' /\ recency_ok c'.
Proof.
  intros Hg Hos. assert (H0 : inv e (Canvas [] None 0 (0, 0) s []) /\ recency_ok (Canvas [] None 0 (0, 0) s [])).
  { split; [split; [simpl; constructor|split; [intros ? ? []|intros ? []]]|split; [constructor|reflexivity]]. }
  revert e Hg Hos H0. generalize (Canvas [] None 0 (0, 0) s []). induction os as [|o os IH]; simpl; intros c e Hg Hos [Hi Hr] H.
  - injection H as <-; auto.
  - destruct (step e c o) as [c1|] eqn:E; [|discriminate].
    assert (Hg' : good (env_after e o)) by (destruct o; simpl; auto; eapply Hos; left; reflexivity).
    apply (IH c1 _ Hg' (fun e' a H' => Hos e' a (or_intror H'))); [|exact H].
    exact (conj (step_keeps_inv _ _ _ _ Hg Hg' Hi (fun _ _ _ _ => proj1 (recency_held _ Hr)) E) (recency_kept _ _ _ _ (proj1 Hi) Hr E)).
Qed.


(* C6: the view moves the least it must, and the focus is on screen whenever its span fits. *)
Definition shows v s len sc := v <= s /\ s + len <= v + sc.

Theorem least_is_least e v s len sc : 0 <= eps e -> len <= sc ->
  shows (least e v s len sc) s len sc /\ forall v', shows v' s len sc -> Z.abs (least e v s len sc - v) <= Z.abs (v' - v).
Proof.
  intros He Hl. unfold shows, least.
  destruct (sc + eps e <? len) eqn:E1, (s <? v) eqn:E2, (v + sc <? s + len) eqn:E3; simpl; bools;
    (split; [lia|intros v' Hv; lia]).
Qed.

Theorem least_keeps_a_view_that_shows e v s len sc : 0 <= eps e -> shows v s len sc -> least e v s len sc = v.
Proof.
  intros He Hs. unfold shows in Hs. unfold least.
  destruct (sc + eps e <? len) eqn:E1, (s <? v) eqn:E2, (v + sc <? s + len) eqn:E3; simpl; bools; lia.
Qed.

(* The focus shown as far as it fits: along an axis it fits on, whole; along one it is larger
   than the screen on, from its start. *)
Definition on_axis e s len sc := if sc + eps e <? len then s = 0 else - eps e <= s /\ s + len <= sc + eps e.
Definition focus_shown e c := forall r, focus_rect c = Some r ->
  on_axis e (ri r * tile_w e (zoom c) - fst (view c)) (rw r * tile_w e (zoom c)) (fst (screen c)) /\
  on_axis e (rj r * tile_h e (zoom c) - snd (view c)) (rh r * tile_h e (zoom c)) (snd (screen c)).
Lemma least_on_axis e v s len sc : 0 <= eps e -> on_axis e (s - least e v s len sc) len sc.
Proof.
  intros He. unfold on_axis, least.
  destruct (sc + eps e <? len) eqn:E1, (s <? v) eqn:E2, (v + sc <? s + len) eqn:E3; simpl; bools; lia.
Qed.
Lemma efv_focus_shown e c : 0 <= eps e -> focus_shown e (ensure_focus_visible e c).
Proof.
  intros He r. unfold ensure_focus_visible. destruct (focus_rect c) as [r0|] eqn:F; intros Hr; [|congruence].
  assert (E : focus_rect (ensure_visible e c r0) = focus_rect c) by reflexivity.
  rewrite E, F in Hr. injection Hr as <-. split; apply least_on_axis, He.
Qed.
Lemma efv_shows e c : 0 <= eps e -> focus_ok e (ensure_focus_visible e c).
Proof.
  intros He r Hr Hf. destruct (efv_focus_shown e c He r Hr) as [X Y]. unfold on_axis, is_visible, fits in *.
  apply andb_true_iff in Hf as [F1 F2]. apply Z.leb_le in F1, F2.
  rewrite (proj2 (Z.ltb_ge _ _) F1) in X. rewrite (proj2 (Z.ltb_ge _ _) F2) in Y. rewrite !andb_true_iff, !Z.leb_le. lia.
Qed.

(* check() keeps the focus on a window (recency_held); rename relies on it, since it checks the
   new id only against the windows. *)
Lemma rename_focus_ok e c i j : focus_known c -> focus_ok e c -> focus_ok e (rename c i j).
Proof.
  intros Hk Hf r. unfold rename. destruct (mem j (ids (windows c))) eqn:Ej; [exact (Hf r)|].
  destruct (window_of i (windows c)) as [w0|]; [|exact (Hf r)]. intros Hr. apply Hf. revert Hr. unfold focus_rect, rect_of; simpl.
  destruct (focus c) as [f|] eqn:F; [|auto]. specialize (Hk f F).
  assert (Hj : ~ In j (ids (windows c))) by (rewrite <- mem_iff; congruence).
  assert (Hfj : f <> j) by (intros ->; contradiction).
  unfold window_of, update. clear w0 Ej F. induction (windows c) as [|w ws IH]; simpl in *; [contradiction|].
  assert (Hj1 : wid w <> j) by tauto. assert (Hj2 : ~ In j (ids ws)) by tauto.
  destruct (Nat.eqb (wid w) f) eqn:D.
  - apply Nat.eqb_eq in D. subst f. destruct (Nat.eqb (wid w) i); simpl; rewrite ?Nat.eqb_refl; auto.
  - destruct Hk as [Hk|Hk]; [apply Nat.eqb_neq in D; contradiction|].
    replace (Nat.eqb (wid (if Nat.eqb (wid w) i then Window j (wrect w) (wmin w) else w)) (if Nat.eqb f i then j else f))
      with false; [intros Hr; apply IH; auto|].
    symmetry. apply Nat.eqb_neq. apply Nat.eqb_neq in D.
    destruct (Nat.eqb (wid w) i) eqn:A, (Nat.eqb f i) eqn:B; simpl; bools; congruence.
Qed.

Theorem focus_stays_visible e c o c' : good (env_after e o) -> focus_ok e c ->
  (forall i j, o = Rename i j -> focus_known c) -> step e c o = Some c' -> focus_ok (env_after e o) c'.
Proof.
  intros (He & _) Hf Hk H. destruct (step_frame _ _ _ _ H) as [[-> E]|[(i & j & -> & ->)|(x & -> & _)]].
  - rewrite E. exact Hf.
  - exact (rename_focus_ok _ _ _ _ (Hk i j eq_refl) Hf).
  - apply efv_shows; exact He.
Qed.

(* C3: a grow that succeeds leaves a span that fits at 1:1, whatever the zoom; no resize leaves
   a window below its need. *)
(* C52: undo and redo keep check()'s invariants: a restored snapshot overlaps nothing and every
   minimum is enforced after it; ids, recent and the focus are kept and the focus is shown. *)
Theorem undo_keeps_check e c l a c' : good e -> inv e c -> recency_ok c -> focus_ok e c -> restore e c l a = Some c' ->
  inv e c' /\ recency_ok c' /\ focus_ok e c'.
Proof.
  intros Hg Hi Hr Hf H. split; [exact (restore_inv _ _ _ _ _ Hg Hi H)|].
  unfold restore in H. destruct (restored (windows c) l) as [ws|] eqn:R; [|injection H as <-; auto].
  destruct (enforce_all e a ws) as [ws'|] eqn:Ea; [|discriminate]. injection H as <-.
  split; [apply recency_efv|apply efv_shows, (proj1 Hg)].
  destruct Hr as [P Fo]. split; simpl; [|exact Fo]. rewrite (enforce_all_ids _ _ _ _ Ea), (restored_ids _ _ _ R). exact P.
Qed.
Theorem grow_fits e c i ed d p c' : 0 < d -> resize e c i ed d p = Some (true, c') ->
  forall r, rect_of i (windows c') = Some r -> fits e 0 (screen c') (rw r) (rh r) = true.
Proof.
  intros Dp H r Hr. destruct (resize_cases _ _ _ _ _ _ _ H) as (w & Hw & Rf & _ & Es & [[_ Br] | [Dn _]]); [|lia].
  destruct (refused_false _ _ _ _ _ Rf) as (_ & _ & Fit). rewrite Es.
  assert (A : Forall2 (same_at i) (update i (set_rect (grown ed (wrect w) d)) (windows c)) (windows c')).
  { destruct Br as [Pl|Hp]; [exact (placed_same_at _ _ _ _ _ Pl)|exact (push_same_at _ _ _ _ Hp)]. }
  pose proof (f2_window_of (same_at i) _ _ i (fun x y H => proj1 H) A) as F.
  rewrite (window_of_update i (set_rect (grown ed (wrect w) d)) _ w (fun _ => eq_refl) Hw) in F.
  unfold rect_of in Hr. destruct (window_of i (windows c')) as [b|]; [|contradiction]. simpl in Hr. injection Hr as <-.
  destruct F as (_ & _ & F3). destruct (F3 (proj2 (window_of_some _ _ _ Hw))) as [F3' F4'].
  simpl in F3', F4'. rewrite <- F3', <- F4'. exact (Fit Dp).
Qed.

Theorem resize_respects_minimum e c i ed d p c' : resize e c i ed d p = Some (true, c') ->
  forall w, window_of i (windows c') = Some w ->
  fst (need e (wmin w)) <= rw (wrect w) /\ snd (need e (wmin w)) <= rh (wrect w).
Proof.
  intros H w' Hw'. destruct (resize_cases _ _ _ _ _ _ _ H) as (w & Hw & Rf & _ & _ & Br).
  destruct (refused_false _ _ _ _ _ Rf) as (Fw & Fh & _).
  assert (A : Forall2 (same_at i) (update i (set_rect (grown ed (wrect w) d)) (windows c)) (windows c')).
  { destruct Br as [[_ [Pl|Hp]] | [_ (freed & side & _ & ->)]]; [exact (placed_same_at _ _ _ _ _ Pl)|exact (push_same_at _ _ _ _ Hp)|].
    apply (f2_impl same); [intros x y _ (E1 & E2 & E3 & E4); unfold same_at; auto|].
    destruct (splits _ _ _); [apply f2_same_refl|apply fill_same]. }
  pose proof (f2_window_of (same_at i) _ _ i (fun x y H => proj1 H) A) as F.
  rewrite (window_of_update i (set_rect (grown ed (wrect w) d)) _ w (fun _ => eq_refl) Hw), Hw' in F.
  destruct F as (_ & F2 & F3). destruct (F3 (proj2 (window_of_some _ _ _ Hw))) as [F3' F4'].
  simpl in F2, F3', F4'. rewrite <- F2, <- F3', <- F4'. auto.
Qed.

(* C4: closing onto ocean moves nothing; a lake fill keeps the canvas disjoint (fill_disjoint). *)
Theorem ocean_shrink_moves_nothing e c i ed d p c' w : resize e c i ed d p = Some (true, c') -> d < 0 ->
  window_of i (windows c) = Some w ->
  chain (fst (strip ed (wrect w) (grown ed (wrect w) d) (- d))) (snd (strip ed (wrect w) (grown ed (wrect w) d) (- d)))
        (update i (set_rect (grown ed (wrect w) d)) (windows c)) = [] ->
  forall x, x <> i -> rect_of x (windows c') = rect_of x (windows c).
Proof.
  intros H Dn Hw Ch x Hx. destruct (resize_cases _ _ _ _ _ _ _ H) as (w' & Hw' & _ & _ & _ & [[Dp _] | [_ (freed & side & St & Ew)]]); [lia|].
  rewrite Hw in Hw'. injection Hw' as <-. rewrite St in Ch. cbn [fst snd] in Ch. rewrite Ew. unfold fill. rewrite Ch.
  match goal with |- context [if ?b then _ else _] => destruct b end; apply rect_of_update; first [reflexivity|exact Hx].
Qed.

Theorem ocean_close_moves_nothing e c i g freed c' : remove e c i g = Some c' -> rect_of i (windows c) = Some freed ->
  is_bridge (windows c) i = false -> chain freed Right (without i (windows c)) = [] -> chain freed Down (without i (windows c)) = [] ->
  windows c' = without i (windows c).
Proof.
  intros H Hr Hb C1 C2. rewrite (proj1 (remove_shape _ _ _ _ _ _ Hr H)). unfold closing, closed, fill. cbv zeta.
  rewrite C1. cbv beta iota. rewrite C2, Hb. destruct (splits _ _ _); reflexivity.
Qed.
(* C7: neighbours share their physical edge, at any scale and view. *)
Theorem neighbours_share_physical_edges (ex ey : Z -> Z) a b :
  (right a = ri b -> let '(xa, _, wa, _) := physical ex ey a in let '(xb, _, _, _) := physical ex ey b in xa + wa = xb) /\
  (bottom a = rj b -> let '(_, ya, _, ha) := physical ex ey a in let '(_, yb, _, _) := physical ex ey b in ya + ha = yb).
Proof. unfold physical. split; intros H; rewrite H; lia. Qed.

(* C36: a zoom that changes the zoom shows the focus as far as it fits, magnified past 1:1 too. *)
Theorem zoom_shows_the_focus e c d : 0 <= eps e -> zoom (zoom_by e c d) <> zoom c -> focus_shown e (zoom_by e c d).
Proof. intros He. unfold zoom_by. destruct (_ =? zoom c); [intros H; contradiction|intros _; apply efv_focus_shown, He]. Qed.

(* C28, C29: add moves no other window, and the new one opens at exactly open_span. *)
Theorem add_moves_no_window e c i m w c' : add e c i m w = Some c' -> exists r, windows c' = windows c ++ [Window i r m].
Proof. intros H. destruct (add_shape _ _ _ _ _ _ H) as (_ & _ & _ & _ & r & E & _). eauto. Qed.
Theorem add_opens_at_open_span e c i m w c' r : add e c i m w = Some c' -> rect_of i (windows c') = Some r ->
  (rw r, rh r) = open_span e (screen c) m w.
Proof.
  intros H Hr. destruct (add_shape _ _ _ _ _ _ H) as (Ni & _ & _ & _ & r0 & E & Hs & _). rewrite E in Hr.
  assert (W : window_of i (windows c ++ [Window i r0 m]) = Some (Window i r0 m)).
  { clear E Hr. revert Ni. generalize (windows c) as ws. unfold window_of, ids.
    induction ws as [|y ws IH]; simpl; intros Ni; [rewrite Nat.eqb_refl; reflexivity|].
    destruct (Nat.eqb (wid y) i) eqn:Ey; bools; [tauto|apply IH; tauto]. }
  unfold rect_of in Hr. rewrite W in Hr. injection Hr as <-. exact Hs.
Qed.

(* C38: a zoom keeps the canvas point under the middle of the screen, (2 v + sc) / (2 t) tiles
   from the origin, to within one unit on screen at the new zoom, whenever showing the focus then
   scrolls nothing: there is no focus, or it lies whole on screen at the rescaled view. *)
Definition keeps_middle t0 t1 sc v v' := 0 <= (2 * v + sc) * t1 - (2 * v' + sc) * t0 < 2 * t0.
Lemma centred_keeps_middle t0 t1 sc v : 0 < t0 -> keeps_middle t0 t1 sc v (centred t0 t1 sc v).
Proof.
  intros H. unfold keeps_middle, centred.
  pose proof (Z.div_mod ((2 * v + sc) * t1 - sc * t0) (2 * t0) ltac:(lia)).
  pose proof (Z.mod_pos_bound ((2 * v + sc) * t1 - sc * t0) (2 * t0) ltac:(lia)). lia.
Qed.
Theorem zoom_keeps_the_middle e c d : 0 <= eps e -> 0 < tile_w e (zoom c) -> 0 < tile_h e (zoom c) ->
  zoom (zoom_by e c d) <> zoom c ->
  (forall r, focus_rect c = Some r -> let z := zoom (zoom_by e c d) in
    shows (fst (rescaled e c z)) (ri r * tile_w e z) (rw r * tile_w e z) (fst (screen c)) /\
    shows (snd (rescaled e c z)) (rj r * tile_h e z) (rh r * tile_h e z) (snd (screen c))) ->
  keeps_middle (tile_w e (zoom c)) (tile_w e (zoom (zoom_by e c d))) (fst (screen c)) (fst (view c)) (fst (view (zoom_by e c d))) /\
  keeps_middle (tile_h e (zoom c)) (tile_h e (zoom (zoom_by e c d))) (snd (screen c)) (snd (view c)) (snd (view (zoom_by e c d))).
Proof.
  intros He Hw Hh. unfold zoom_by. destruct (_ =? zoom c); [intros N; contradiction|intros _]. efv. cbn [zoom].
  set (z := Z.max _ _). intros Hs. unfold ensure_focus_visible.
  assert (F : focus_rect (Canvas (windows c) (focus c) z (rescaled e c z) (screen c) (recent c)) = focus_rect c) by reflexivity.
  rewrite F. destruct (focus_rect c) as [r|] eqn:Fr.
  - destruct (Hs r eq_refl) as [Sx Sy]. unfold ensure_visible, with_view; cbn [view screen zoom fst snd].
    rewrite !least_keeps_a_view_that_shows by assumption.
    split; apply centred_keeps_middle; assumption.
  - split; apply centred_keeps_middle; assumption.
Qed.

(* C39: pulling the camera back d steps and coming closer again gives back the canvas, when
   neither zoom is clamped, the view shows the focus as least leaves it (any operation but scroll_by
   and set_view ends there), the tiles shrink by at least two units' worth over the screen on each
   axis (2 t0 <= sc (t0 - t1)), and the rescale lands on the model's unit (f64 has no such floor):
   pulled back, a whole focus stays whole without a scroll, and closer again, the opposite rescale
   undoes the first. Coming closer first returns only when it does not scroll (C38's premise). *)
Lemma centred_back t0 t1 sc v : 0 < t0 -> 0 < t1 -> ((2 * v + sc) * t1 - sc * t0) mod (2 * t0) = 0 ->
  centred t1 t0 sc (centred t0 t1 sc v) = v.
Proof.
  intros H0 H1 Hm. unfold centred at 2.
  pose proof (Z.div_mod ((2 * v + sc) * t1 - sc * t0) (2 * t0) ltac:(lia)) as E. rewrite Hm in E.
  set (q := ((2 * v + sc) * t1 - sc * t0) / (2 * t0)) in *. unfold centred.
  replace ((2 * q + sc) * t0 - sc * t1) with (v * (2 * t1)) by lia. apply Z.div_mul. lia.
Qed.
Lemma centred_shows t0 t1 sc v s len : 0 < t1 <= t0 -> 2 * t0 <= sc * (t0 - t1) ->
  shows v (s * t0) (len * t0) sc -> shows (centred t0 t1 sc v) (s * t1) (len * t1) sc.
Proof.
  intros [H1 H10] Hm [A B]. unfold shows, centred.
  pose proof (Z.div_mod ((2 * v + sc) * t1 - sc * t0) (2 * t0) ltac:(lia)) as E.
  pose proof (Z.mod_pos_bound ((2 * v + sc) * t1 - sc * t0) (2 * t0) ltac:(lia)) as M.
  set (q := ((2 * v + sc) * t1 - sc * t0) / (2 * t0)) in *.
  assert (P1 : 0 <= t1 * (s * t0 - v)) by nia. assert (P2 : 0 <= t1 * (v + sc - s * t0 - len * t0)) by nia.
  split; nia.
Qed.
Lemma axis_out_and_back e t0 t1 sc v s len : 0 <= eps e -> 0 < t1 <= t0 -> 2 * t0 <= sc * (t0 - t1) ->
  ((2 * v + sc) * t1 - sc * t0) mod (2 * t0) = 0 -> least e v (s * t0) (len * t0) sc = v ->
  least e (centred t1 t0 sc (least e (centred t0 t1 sc v) (s * t1) (len * t1) sc)) (s * t0) (len * t0) sc = v.
Proof.
  intros He Ht Hm Hx Hv. unfold least at 1. unfold least in Hv.
  destruct (sc + eps e <? len * t0) eqn:O; simpl in *; [exact Hv|].
  assert (S : shows v (s * t0) (len * t0) sc)
    by (unfold shows; destruct (s * t0 <? v) eqn:A, (v + sc <? s * t0 + len * t0) eqn:B; simpl in Hv; bools; lia).
  rewrite (least_keeps_a_view_that_shows e _ _ _ _ He (centred_shows _ _ _ _ _ _ Ht Hm S)).
  rewrite centred_back by (assumption || lia).
  destruct (s * t0 <? v) eqn:A, (v + sc <? s * t0 + len * t0) eqn:B; simpl; bools; lia.
Qed.
Lemma zoom_by_unclamped e c d z : Z.max (zoom_min e) (Z.min (zoom_max e) (zoom c + d)) = z -> z <> zoom c ->
  zoom_by e c d = Canvas (windows c) (focus c) z (match focus_rect c with
    | Some r => (least e (fst (rescaled e c z)) (ri r * tile_w e z) (rw r * tile_w e z) (fst (screen c)),
                 least e (snd (rescaled e c z)) (rj r * tile_h e z) (rh r * tile_h e z) (snd (screen c)))
    | None => rescaled e c z end) (screen c) (recent c).
Proof.
  intros Hz Hn. unfold zoom_by. rewrite Hz, (proj2 (Z.eqb_neq _ _) Hn). unfold ensure_focus_visible.
  change (focus_rect (Canvas (windows c) (focus c) z (rescaled e c z) (screen c) (recent c))) with (focus_rect c).
  destruct (focus_rect c); reflexivity.
Qed.
Theorem zoom_out_and_back e c d : 0 <= eps e -> 0 < d -> zoom_min e <= zoom c -> zoom c + d <= zoom_max e ->
  ensure_focus_visible e c = c ->
  0 < tile_w e (zoom c + d) <= tile_w e (zoom c) -> 2 * tile_w e (zoom c) <= fst (screen c) * (tile_w e (zoom c) - tile_w e (zoom c + d)) ->
  0 < tile_h e (zoom c + d) <= tile_h e (zoom c) -> 2 * tile_h e (zoom c) <= snd (screen c) * (tile_h e (zoom c) - tile_h e (zoom c + d)) ->
  ((2 * fst (view c) + fst (screen c)) * tile_w e (zoom c + d) - fst (screen c) * tile_w e (zoom c)) mod (2 * tile_w e (zoom c)) = 0 ->
  ((2 * snd (view c) + snd (screen c)) * tile_h e (zoom c + d) - snd (screen c) * tile_h e (zoom c)) mod (2 * tile_h e (zoom c)) = 0 ->
  zoom_by e (zoom_by e c d) (-d) = c.
Proof.
  intros He Hd Hlo Hhi Hc Tw Mw Th Mh Ex Ey.
  destruct c as [ws f z0 [vx vy] [sx sy] rc]. cbn [zoom view screen fst snd] in *.
  rewrite (zoom_by_unclamped e _ d (z0 + d)) by (cbn; lia).
  rewrite (zoom_by_unclamped e _ (-d) z0) by (cbn; lia).
  unfold ensure_focus_visible, focus_rect in *. cbn [windows focus zoom view screen recent fst snd] in *.
  unfold rescaled. cbn [windows focus zoom view screen recent fst snd].
  destruct (match f with Some i => rect_of i ws | None => None end) as [r|].
  - unfold ensure_visible, with_view in Hc. cbn [windows focus zoom view screen recent fst snd] in Hc.
    injection Hc as Lx Ly. cbn [fst snd]. rewrite !axis_out_and_back by (assumption || lia). reflexivity.
  - cbn [fst snd]. rewrite !centred_back by (assumption || lia). reflexivity.
Qed.

(* C41: C38's and C39's premises are met: three tiles in a row on 2560x1440 (binds_the_middle),
   pulled back one step, keep the middle, and coming closer again gives back the canvas. *)
Definition three := match run shipped (canvas_new 2560 1440) [Add 1 (100, 100) None; Add 2 (100, 100) None;
  Add 3 (100, 100) None] with Some c => c | None => canvas_new 0 0 end.
Example middle_premises_met : zoom_by shipped (zoom_by shipped three 1) (-1) = three /\
  keeps_middle (tile_w shipped (zoom three)) (tile_w shipped (zoom (zoom_by shipped three 1))) (fst (screen three))
    (fst (view three)) (fst (view (zoom_by shipped three 1))).
Proof.
  split.
  - apply (zoom_out_and_back shipped three 1); vm_compute; repeat split; try reflexivity; intro H; discriminate H.
  - refine (proj1 (zoom_keeps_the_middle shipped three 1 _ _ _ _ _)); 
      [vm_compute; intro H; discriminate H|vm_compute; reflexivity|vm_compute; reflexivity|vm_compute; intro H; discriminate H|].
    assert (F : focus_rect three = Some (Rect 2 0 1 1)) by (vm_compute; reflexivity).
    rewrite F. intros r Hr. injection Hr as <-. vm_compute. repeat split; intro H; discriminate H.
Qed.
