(* The span principle: a window's span changes only through a resize or its minimum. Moving it,
   by whatever operation, keeps it; a move made of a remove and an add would not (F04). *)
From Stdlib Require Import ZArith List Bool Lia.
Import ListNotations.
From NewmCanvas Require Import Canvas Push Claims.
Open Scope Z_scope.

Definition span w := (rw (wrect w), rh (wrect w)).
(* After an operation that is no resize, a window's span is the span it had, raised to what its
   minimum needs under the env the operation leaves. *)
Definition raised e (w w' : window) :=
  span w' = (Z.max (rw (wrect w)) (fst (need e (wmin w'))), Z.max (rh (wrect w)) (snd (need e (wmin w')))).
Definition kept e ws ws' := forall w w', In w ws -> In w' ws' -> wid w = wid w' -> raised e w w'.

Lemma kept_f2 e (R : window -> window -> Prop) ws ws' : NoDup (ids ws) ->
  (forall a b, R a b -> wid a = wid b) -> (forall a b, In a ws -> R a b -> raised e a b) ->
  Forall2 R ws ws' -> kept e ws ws'.
Proof.
  intros Hn Hid Hr A w w' Hw Hw' E. destruct (f2_in_r _ _ _ _ A Hw') as (x & Hx & Rx).
  assert (x = w) as -> by (apply (nodup_unique ws); auto; rewrite (Hid _ _ Rx); congruence). auto.
Qed.
Lemma same_raised e ws a b : mins_ok e ws -> In a ws -> same a b -> raised e a b.
Proof.
  intros Hm Ha (_ & E2 & E3 & E4). destruct (Hm a Ha). unfold raised, span. rewrite <- E2, <- E3, <- E4. f_equal; lia.
Qed.
Lemma kept_same e ws l ws' : NoDup (ids ws) -> mins_ok e ws -> (forall x, In x l -> In x ws) ->
  Forall2 same l ws' -> kept e ws ws'.
Proof.
  intros Hn Hm Hl A w w' Hw Hw' E. destruct (f2_in_r _ _ _ _ A Hw') as (x & Hx & Sx).
  assert (x = w) as -> by (apply (nodup_unique ws); auto; destruct Sx; congruence).
  exact (same_raised e ws w w' Hm Hw Sx).
Qed.
Lemma kept_refl e ws : NoDup (ids ws) -> mins_ok e ws -> kept e ws ws.
Proof. intros Hn Hm. exact (kept_same e ws ws ws Hn Hm (fun _ h => h) (f2_same_refl ws)). Qed.
Lemma kept_snoc e ws i r m : NoDup (ids ws) -> mins_ok e ws -> ~ In i (ids ws) -> kept e ws (ws ++ [Window i r m]).
Proof.
  intros Hn Hm Hi w w' Hw Hw' E. apply in_app_or in Hw' as [Hw'|[<-|[]]]; [exact (kept_refl e ws Hn Hm w w' Hw Hw' E)|].
  exfalso. apply Hi. simpl in E. rewrite <- E. apply in_map, Hw.
Qed.
(* A window taken out and put back in one operation keeps its span: the rest is filled around it. *)
Lemma take_same i ws w rest x : NoDup (ids ws) -> take i ws = Some (w, rest) -> same w x ->
  Forall2 same (without i ws ++ [w]) (rest ++ [x]) /\ (forall y, In y (without i ws ++ [w]) -> In y ws).
Proof.
  intros Hn T S. unfold take in T. destruct (window_of i ws) as [w0|] eqn:Hw; [|discriminate].
  injection T as <- <-. apply window_of_some in Hw as [Hwin _]. destruct (without_ok i ws Hn) as (_ & _ & N3).
  split.
  - apply Forall2_app; [|constructor; [exact S|constructor]]. unfold closed. eapply f2_same_trans; apply fill_same.
  - intros y Hy. apply in_app_or in Hy as [Hy|[<-|[]]]; auto.
Qed.

(* Every operation but a resize keeps each window's span, raised only to what its minimum needs
   under the env it leaves: set_min and set_config raise it to the new need, never more, and every
   other operation, swap included, keeps it as it was. *)
Lemma kept_then_same e ws l l' : kept e ws l -> Forall2 same l l' -> kept e ws l'.
Proof.
  intros K A a b Ha Hb E. destruct (f2_in_r _ _ _ _ A Hb) as (z & Hz & (S1 & S2 & S3 & S4)).
  pose proof (K a z Ha Hz ltac:(congruence)) as R. unfold raised, span in *. rewrite <- S2, <- S3, <- S4. exact R.
Qed.
Lemma kept_unless_resized e c o c' : good e -> good (env_after e o) -> inv e c ->
  (forall i ed d p, o <> Resize i ed d p) -> (forall l a, o <> Restore l a) -> step e c o = Some c' ->
  kept (env_after e o) (windows c) (windows c').
Proof.
  intros Hg Hg' (Hn & Hd & Hm) Hr Hl H. destruct o; cbn [step env_after] in *.
  - destruct (add_shape _ _ _ _ _ _ H) as (Ni & _ & _ & _ & r & E & _). rewrite E. exact (kept_snoc _ _ _ _ _ Hn Hm Ni).
  - unfold move_by in H. destruct (window_of i (windows c)) as [w|] eqn:Hw; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (placed_by _ _ p) as [ws|] eqn:Pl.
    { injection H as <-. efv. cbn [windows with_windows]. apply (kept_same _ _ (windows c) _ Hn Hm (fun _ h => h)).
      exact (placed_same _ _ _ _ Pl (one_target _ _ _ (shifted (wrect w) (fst (delta d)) (snd (delta d))) Hn Hw eq_refl eq_refl)). }
    unfold move_by_push in H. rewrite Hw in H.
    destruct (push _ _ _) as [ws|] eqn:Hp; [|discriminate]. injection H as <-. efv. cbn [windows with_windows].
    apply window_of_some in Hw as [Hw0 Ew0].
    lazymatch type of Hp with push _ _ ?u = _ =>
      assert (A : Forall2 same (windows c) u) end.
    { apply update_f2; intros z Hz E; [|unfold same; auto].
      assert (z = w) as -> by (apply (nodup_unique (windows c)); auto; congruence). unfold same; simpl; auto. }
    apply (kept_same _ _ (windows c) _ Hn Hm (fun _ h => h)). eapply f2_same_trans; [exact A|].
    apply (push_same _ _ _ _ ltac:(rewrite <- (same_ids _ _ A); exact Hn) Hp).
  - destruct (rect_of i (windows c)) as [freed|] eqn:Hf;
      [|unfold remove in H; rewrite Hf in H; injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (remove_shape _ _ _ _ _ _ Hf H) as (E & _). rewrite E. destruct (without_ok i _ Hn) as (_ & _ & N3).
    apply (kept_same _ _ (without i (windows c)) _ Hn Hm N3). exact (proj1 (closing_same _ _ _ _)).
  - injection H as <-. unfold focus_id. destruct (mem _ _); [efv; unfold set_focus; cbn [windows]|]; exact (kept_refl _ _ Hn Hm).
  - exfalso. eapply Hr; reflexivity.
  - unfold swap in H. destruct (focus c) as [f|] eqn:Ef; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (Nat.eqb f n) eqn:Efn; [injection H as <-; exact (kept_refl _ _ Hn Hm)|].
    destruct (rect_of f (windows c)) as [ra|] eqn:Ra; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (window_of n (windows c)) as [wn|] eqn:Wn; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (swapped d ra (wrect wn)) as [fa nb] eqn:Sw.
    destruct (placed_by _ _ p) as [ws|] eqn:Pl.
    { injection H as <-. efv. cbn [windows with_windows]. apply (kept_same _ _ (windows c) _ Hn Hm (fun _ h => h)).
      exact (placed_same _ _ _ _ Pl (swap_targets _ _ _ _ _ _ _ _ Hn Ra Wn Sw)). }
    unfold swap_push in H. rewrite Ef, Efn, Ra, Wn, Sw in H. cbv beta iota zeta in H.
    destruct (push d f _) as [ws1|] eqn:P1; [|discriminate].
    destruct (push (opposite d) n _) as [ws2|] eqn:P2; [|discriminate].
    injection H as <-. efv. cbn [windows with_windows].
    destruct (swap_ok _ _ _ _ _ _ _ _ _ _ Hn (mins_spans _ _ Hg Hm) Ra Wn Sw P1 P2) as (_ & _ & A & I).
    exact (kept_same _ _ _ _ Hn Hm I A).
  - unfold send_to_edge in H. destruct (focus c) as [f|] eqn:Ef; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (_ || _) eqn:G; [injection H as <-; exact (kept_refl _ _ Hn Hm)|].
    assert (Fb : send_push e c d = Some c' -> kept e (windows c) (windows c')).
    { clear H. intros H. unfold send_push in H. rewrite Ef, G in H.
      destruct (take f (windows c)) as [[w rest]|] eqn:T; [|discriminate].
      destruct rest as [|o rest']; [discriminate|]. cbv zeta in H. injection H as <-. efv. cbn [windows with_windows].
      lazymatch goal with |- context [_ ++ [?x]] => assert (S : same w x) by (unfold same; simpl; auto) end.
      destruct (take_same _ _ _ _ _ Hn T S) as (A & I). exact (kept_same _ _ _ _ Hn Hm I A). }
    destruct (window_of f (windows c)) as [w|] eqn:Hw; [|exact (Fb H)].
    destruct (without f (windows c)) as [|o rest]; [exact (Fb H)|].
    destruct (placed_by _ _ p) as [ws|] eqn:Pl; [|exact (Fb H)].
    injection H as <-. efv. cbn [windows with_windows]. apply (kept_same _ _ (windows c) _ Hn Hm (fun _ h => h)).
    exact (placed_same _ _ _ _ Pl (one_target _ _ _ (Rect (edge_i d w o rest) (rj (wrect w)) (rw (wrect w)) (rh (wrect w))) Hn Hw eq_refl eq_refl)).
  - unfold move_to in H. destruct (rect_of i (windows c)) as [r|] eqn:Hr0; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (rect_eqb _ r); [injection H as <-; exact (kept_refl _ _ Hn Hm)|].
    assert (G : forall u, kept e (windows c) u -> kept e (windows c) (if is_bridge (windows c) i then gathered u g else u)).
    { intros u K. destruct (is_bridge _ _); [exact (kept_then_same _ _ _ _ K (proj1 (gathered_ok u g)))|exact K]. }
    destruct (placed_by _ _ p) as [u|] eqn:Pl.
    { injection H as <-. efv. cbn [windows with_windows]. apply G. apply (kept_same _ _ (windows c) _ Hn Hm (fun _ h => h)).
      destruct (rect_of_some _ _ _ Hr0) as (w & Hwin & Hwid & Hwr).
      exact (placed_same _ _ _ _ Pl (one_target _ _ _ (Rect x y (rw r) (rh r)) Hn (window_of_in _ _ _ Hn Hwin Hwid)
        ltac:(rewrite Hwr; reflexivity) ltac:(rewrite Hwr; reflexivity))). }
    destruct (moved_to i x y (windows c)) as [ws|] eqn:Mt; [|discriminate]. injection H as <-. efv. cbn [windows with_windows]. apply G.
    unfold moved_to in Mt. destruct (take i (windows c)) as [[w rest]|] eqn:T; [|injection Mt as <-; exact (kept_refl _ _ Hn Hm)].
    rename Mt into Hp.
    lazymatch type of Hp with push _ _ (_ ++ [?x]) = _ => assert (S : same w x) by (unfold same; simpl; auto) end.
    destruct (take_same _ _ _ _ _ Hn T S) as (A & I).
    destruct (take_ok e _ _ _ _ Hn Hd Hm T) as (Hw & N1 & N2 & _). apply window_of_some in Hw as [_ Ew].
    apply (kept_same _ _ _ _ Hn Hm I). eapply f2_same_trans; [exact A|]. refine (push_same _ _ _ _ _ Hp).
    unfold ids; rewrite map_app. apply nodup_snoc; [exact N1|]. destruct S as (<- & _). rewrite Ew. exact N2.
  - injection H as <-. unfold zoom_by. cbv zeta. destruct (_ =? _); [|efv; cbn [windows]]; exact (kept_refl _ _ Hn Hm).
  - rename a into ans. unfold set_min in H. destruct (window_of i (windows c)) as [w0|] eqn:Hw; [|injection H as <-; exact (kept_refl _ _ Hn Hm)].
    destruct (enforce_min e _ i _) as [ws|] eqn:He; [|discriminate]. injection H as <-. efv. cbn [windows with_windows].
    set (U := fun a b : window => wid a = wid b /\ wrect a = wrect b /\ (wid a <> i -> wmin a = wmin b)).
    set (u := update i (fun w => Window (wid w) (wrect w) m) (windows c)) in He.
    assert (A : Forall2 U (windows c) u).
    { apply update_f2; intros z Hz E; unfold U; simpl; repeat split; intros; congruence. }
    assert (I1 : ids u = ids (windows c)) by (symmetry; apply (f2_ids U); [intros ? ? (? & _); auto|exact A]).
    assert (S1 : spans_ok u).
    { intros y Hy. destruct (f2_in_r _ _ _ _ A Hy) as (z & Hz & (_ & Ez & _)). rewrite <- Ez. exact (mins_spans _ _ Hg Hm z Hz). }
    assert (D1 : disjoint u) by (apply (update_rects_disjoint i (fun w => Window (wid w) (wrect w) m) _ (fun w => conj eq_refl eq_refl) Hd)).
    destruct (enforce_min_ok _ _ _ _ _ Hg ltac:(rewrite I1; exact Hn) S1 D1 He) as (_ & _ & _ & B).
    apply (kept_f2 e (fun a b => exists z, U a z /\ grew e i z b) _ _ Hn).
    + intros a b (z & (K1 & _) & (F1 & _)). congruence.
    + intros a b Ha (z & (K1 & K2 & K3) & (F1 & F2 & F3 & F4)). destruct (Hm a Ha). unfold raised, span.
      rewrite F3, F4, <- F2, <- K2. destruct (Nat.eqb (wid z) i) eqn:Ez; bools; [reflexivity|].
      rewrite <- (K3 ltac:(congruence)). f_equal; lia.
    + revert A B. apply f2_trans. intros a z b A B. exists z. auto.
  - injection H as <-. unfold set_screen. efv. cbn [windows]. exact (kept_refl _ _ Hn Hm).
  - rename a into ans. unfold set_config in H. cbv zeta in H. destruct (enforce_all e0 _ (windows c)) as [ws|] eqn:He; [|discriminate].
    injection H as <-. efv. cbn [windows].
    destruct (enforce_all_ok _ _ _ _ Hg' Hn (mins_spans _ _ Hg Hm) Hd He) as (_ & _ & _ & A).
    apply (kept_f2 e0 (grew_all e0 (ids (windows c))) _ _ Hn); [intros a b (E & _); exact E| |exact A].
    intros a b Ha (K1 & K2 & K3 & K4). unfold raised, span.
    assert (M : mem (wid a) (ids (windows c)) = true) by (apply mem_iff, in_map, Ha).
    rewrite M in K3, K4. rewrite K3, K4, K2. reflexivity.
  - injection H as <-. unfold rename. destruct (mem j _) eqn:Mj; [exact (kept_refl _ _ Hn Hm)|].
    destruct (window_of i _) as [w0|]; [|exact (kept_refl _ _ Hn Hm)]. cbn [windows].
    intros w w' Hw Hw' E. unfold update in Hw'. apply in_map_iff in Hw' as (z & <- & Hz).
    destruct (Nat.eqb (wid z) i) eqn:Ez; simpl in E.
    + exfalso. assert (Ij : mem j (ids (windows c)) = true) by (apply mem_iff; rewrite <- E; apply in_map, Hw). congruence.
    + assert (z = w) as -> by (apply (nodup_unique (windows c)); auto; congruence).
      exact (same_raised e _ w w Hm Hw (conj eq_refl (conj eq_refl (conj eq_refl eq_refl)))).
  - destruct (add_at_shape _ _ _ _ _ _ H) as (Ni & _). destruct (add_at_moves_no_window _ _ _ _ _ _ H) as (r & E).
    rewrite E. exact (kept_snoc _ _ _ _ _ Hn Hm Ni).
  - exfalso. eapply Hl; reflexivity.
  - injection H as <-. unfold gather_islands. efv. cbn [windows with_windows].
    exact (kept_then_same _ _ _ _ (kept_refl _ _ Hn Hm) (proj1 (gathered_ok (windows c) g))).
Qed.

(* A resize changes no span but the resized window's: a grow places or pushes the others at their
   spans, a shrink fills around it with theirs. *)
Definition same_but i (a b : window) := wid a = wid b /\ wmin a = wmin b /\
  (wid a <> i -> rw (wrect a) = rw (wrect b) /\ rh (wrect a) = rh (wrect b)).
Lemma regrown_same_but i r a b x : regrown i r a b -> same b x -> same_but i a x.
Proof.
  intros (E1 & E2 & E3) (S1 & S2 & S3 & S4). split; [congruence|split; [congruence|]].
  intros Ne. destruct (Nat.eqb (wid a) i) eqn:E; [apply Nat.eqb_eq in E; contradiction|].
  destruct E3 as [E3 E4]. split; congruence.
Qed.
Lemma resize_keeps_the_others e c i ed d p c' : NoDup (ids (windows c)) ->
  option_map snd (resize e c i ed d p) = Some c' -> Forall2 (same_but i) (windows c) (windows c').
Proof.
  intros Hn H. destruct (resize e c i ed d p) as [[[|] c'']|] eqn:R; simpl in H; [| |discriminate]; injection H as <-.
  2: { unfold resize in R. destruct (window_of i (windows c)); [destruct (refused _ _ _ _ _)|];
       cbv zeta in R; try (injection R as <-; apply f2_diag; intros; split; auto).
       destruct (0 <? d); [destruct (placed_by _ _ _); [|destruct (push _ _ _)]|destruct (strip _ _ _ _)]; discriminate. }
  destruct (resize_cases _ _ _ _ _ _ _ R) as (w & Hw & _ & _ & _ & Br). set (new := grown ed (wrect w) d) in *.
  pose proof (update_regrown i new (windows c)) as U.
  assert (Un : NoDup (ids (update i (set_rect new) (windows c)))) by (rewrite update_ids; auto).
  assert (Same : forall l, Forall2 same (update i (set_rect new) (windows c)) l -> Forall2 (same_but i) (windows c) l)
    by (intros l A; exact (f2_trans _ _ _ _ _ _ (fun a b x R0 S0 => regrown_same_but _ _ _ _ _ R0 S0) U A)).
  destruct Br as [[_ [Pl|Hp]] | [_ (freed & side & _ & ->)]].
  - destruct (placed_layouts_keep_the_rules _ _ _ _ Pl) as (A & _). revert A. apply f2_impl.
    intros a b _ (E1 & E2 & E3). split; [exact E1|split; [exact E2|]]. intros Ne.
    destruct (target_of (wid a) [(i, new)]) as [t|] eqn:T.
    + apply target_in in T as [T|[]]. injection T as T1 _. exfalso. exact (Ne (eq_sym T1)).
    + destruct E3 as [E3 E4]. split; congruence.
  - exact (Same _ (push_same _ _ _ _ Un Hp)).
  - destruct (splits _ _ _); apply Same; [apply f2_same_refl|apply fill_same].
Qed.

(* The span principle over one operation: a window keeps its span, raised only to its need, unless
   the operation resizes it; a renamed window is followed to its new id. *)
Definition resized o := match o with Resize i _ _ _ => Some i | _ => None end.
Definition renamed o c k := match o with
  | Rename i j => if mem j (ids (windows c)) then k else if Nat.eqb k i then j else k
  | _ => k end.
Definition kept_by e (ren : nat -> nat) (x : option nat) ws ws' :=
  forall w w', In w ws -> In w' ws' -> wid w' = ren (wid w) -> x <> Some (wid w) -> raised e w w'.
Lemma kept_by_kept e ws ws' : kept e ws ws' -> kept_by e (fun k => k) None ws ws'.
Proof. intros K w w' Hw Hw' E _. exact (K w w' Hw Hw' (eq_sym E)). Qed.
Lemma same_but_kept e i ws ws' : NoDup (ids ws) -> mins_ok e ws -> Forall2 (same_but i) ws ws' ->
  kept_by e (fun k => k) (Some i) ws ws'.
Proof.
  intros Hn Hm A w w' Hw Hw' E Ne. cbv beta in E. destruct (f2_in_r _ _ _ _ A Hw') as (x & Hx & (S1 & S2 & S3)).
  assert (x = w) as -> by (apply (nodup_unique ws); auto; congruence).
  destruct (S3 (fun Ei => Ne (f_equal Some (eq_sym Ei)))) as [S4 S5].
  exact (same_raised e ws w w' Hm Hw (conj S1 (conj S2 (conj S4 S5)))).
Qed.
Lemma rename_keeps_span e c i j : NoDup (ids (windows c)) -> mins_ok e (windows c) ->
  kept_by e (renamed (Rename i j) c) None (windows c) (windows (rename c i j)).
Proof.
  intros Hn Hm w w' Hw Hw' E _. cbn [renamed] in E. unfold rename in Hw'.
  assert (Kw : raised e w w) by exact (same_raised e _ w w Hm Hw (conj eq_refl (conj eq_refl (conj eq_refl eq_refl)))).
  destruct (mem j (ids (windows c))) eqn:Mj.
  { assert (w' = w) as -> by (apply (nodup_unique (windows c)); auto). exact Kw. }
  assert (Nj : forall z, In z (windows c) -> wid z <> j).
  { intros z Hz Ez. assert (Ij : In j (ids (windows c))) by (rewrite <- Ez; apply in_map, Hz).
    apply mem_iff in Ij. congruence. }
  destruct (window_of i (windows c)) as [w0|] eqn:W0.
  - cbn [windows] in Hw'. unfold update in Hw'. apply in_map_iff in Hw' as (z & <- & Hz).
    destruct (Nat.eqb (wid w) i) eqn:Ew, (Nat.eqb (wid z) i) eqn:Ez; simpl in E.
    + apply Nat.eqb_eq in Ew, Ez. assert (z = w) as -> by (apply (nodup_unique (windows c)); auto; congruence).
      destruct (Hm w Hw). unfold raised, span. simpl. f_equal; lia.
    + exfalso. exact (Nj z Hz E).
    + exfalso. exact (Nj w Hw (eq_sym E)).
    + assert (z = w) as -> by (apply (nodup_unique (windows c)); auto). exact Kw.
  - destruct (Nat.eqb (wid w) i) eqn:Ew.
    + apply Nat.eqb_eq in Ew. rewrite (window_of_in _ _ _ Hn Hw Ew) in W0. discriminate.
    + assert (w' = w) as -> by (apply (nodup_unique (windows c)); auto). exact Kw.
Qed.

(* C47: the span principle. Whatever an operation does, a window it does not resize keeps its
   span, raised only to what its minimum needs under the env the operation leaves: set_min and
   set_config raise it to the new need, never more; a resize changes no span but its window's; a
   rename keeps the renamed window's; every other operation keeps every span as it was. Undo and
   redo (Restore) put back a snapshot's spans. *)
Theorem spans_change_only_by_resize e c o c' : good e -> good (env_after e o) -> inv e c ->
  (forall l a, o <> Restore l a) -> step e c o = Some c' ->
  kept_by (env_after e o) (renamed o c) (resized o) (windows c) (windows c').
Proof.
  intros Hg Hg' Hi Hl H. pose proof Hi as (Hn & _ & Hm).
  destruct o; cbn [renamed resized];
    try (apply kept_by_kept; refine (kept_unless_resized _ _ _ _ Hg Hg' Hi _ Hl H); intros; discriminate).
  - cbn [step env_after] in *. exact (same_but_kept _ _ _ _ Hn Hm (resize_keeps_the_others _ _ _ _ _ _ _ Hn H)).
  - cbn [step env_after] in *. injection H as <-. exact (rename_keeps_span _ _ _ _ Hn Hm).
Qed.

(* C48: a tile that leaves its canvas carries its span (src/layout/canvas/moves.rs carry_span),
   and the canvas it arrives on opens it at exactly that span when the span fits that screen at
   1:1 and holds what the minimum needs; otherwise open_span cuts it to fit, never below need. *)
Theorem carried_span_opens e c i m w h c' r : 0 < tile_w e 0 -> 0 < tile_h e 0 ->
  fst (need e m) <= w -> snd (need e m) <= h ->
  w * tile_w e 0 <= fst (screen c) + eps e -> h * tile_h e 0 <= snd (screen c) + eps e ->
  add e c i m (Some (w, h)) = Some c' -> rect_of i (windows c') = Some r -> (rw r, rh r) = (w, h).
Proof.
  intros Tw Th Nw Nh Fw Fh H Hr. rewrite (add_opens_at_open_span _ _ _ _ _ _ _ H Hr). unfold open_span.
  destruct (need e m) as [nw nh] eqn:En. simpl in Nw, Nh. cbn beta iota.
  assert (w <= (fst (screen c) + eps e) / tile_w e 0) by (apply Z.div_le_lower_bound; lia).
  assert (h <= (snd (screen c) + eps e) / tile_h e 0) by (apply Z.div_le_lower_bound; lia).
  f_equal; lia.
Qed.

(* C48's premises met, and the cut: on 2560x1440 a 2x2 tile arrives 2x2; a 4x2 one, wider than the
   three 720 px tiles the screen holds at 1:1, arrives 3x2. *)
Example carried_spans :
  option_map (fun c => rect_of 1 (windows c)) (add shipped (canvas_new 2560 1440) 1 (100, 100) (Some (2, 2)))
    = Some (Some (Rect 0 0 2 2)) /\
  option_map (fun c => rect_of 1 (windows c)) (add shipped (canvas_new 2560 1440) 1 (100, 100) (Some (4, 2)))
    = Some (Some (Rect 0 0 3 2)) /\
  0 < tile_w shipped 0 /\ 0 < tile_h shipped 0 /\ 2 * tile_w shipped 0 <= 25600000 + eps shipped /\
  2 * tile_h shipped 0 <= 14400000 + eps shipped /\ need shipped (100, 100) = (1, 1).
Proof. vm_compute. repeat split; congruence. Qed.
