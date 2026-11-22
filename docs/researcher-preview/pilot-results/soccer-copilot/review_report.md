# SOTA Review Report: soccer statistics, results prediction and analytics and how they influence matches

- **Source Mode:** Live Provider-Native (copilot-cli)

## Challenged / Unsupported Claims
- The comparative groups are bibliographically supplied groupings, not a verified thematic or evidentiary synthesis.
- Seminal-work claims are unsupported: the entries rely on model recall and provide no abstracts, full texts, methods, results, or citation evidence.
- The claim that Dixon and Coles introduced an "influential" dynamic score model is unverified; "influential" should not be used until impact evidence is supplied.
- Claiming that Rue and Salvesen developed the described dynamic Bayesian framework is unverified without the paper's methodology.
- Claiming that Karlis and Ntzoufras improve probabilistic treatment of scores is unverified without an empirical comparison or results.
- The described methods and advantages attributed to Baio and Blangiardo, Hvattum and Arntzen, Koopman and Lit, Groll and colleagues, and Anzer and Bauer are not established by the supplied evidence.
- The statement that Mackenzie and Cushion demonstrate limitations or actionable-insight challenges is unsupported without a full-text review.
- The claims that Pappalardo and colleagues provide an open, reproducible event resource and that Rein and Memmert discuss tactical-data limits are not verified from the supplied metadata.
- Describing Decroos and colleagues' VAEP framework as extending valuation beyond goals and assists is a recalled substantive claim that requires source verification.
- No supplied entry has verified research contribution, reported results, or limitations; marking these fields NOT_VERIFIED is therefore appropriate rather than merely precautionary.

## Missing Foundational Works
- Foundational Bradley-Terry and Elo-rating literature underlying the team-strength and match-prediction entries, including the original rating and paired-comparison works rather than only later football applications.
- Foundational independent-goals and Poisson regression work in football, to establish whether Maher 1982 is being distinguished from later correlated-score and dynamic-strength models.
- Foundational betting-market and forecast-calibration work is missing, so Dixon and Coles 1997 cannot be contextualized against market efficiency, probability scoring, or betting profitability literature.
- Foundational work on bivariate Poisson and dependence modelling is missing, including the methodological precursors that establish the independent contribution of Karlis and Ntzoufras 2003.
- Foundational football performance-indicator research predating Mackenzie and Cushion 2013 is missing; at minimum, source-verified papers on possession, shots, and outcome discrimination are needed before the later reviews can be characterized.
- Foundational positional tracking and optical-tracking systems are missing, so Rein and Memmert 2016 and Memmert and colleagues 2017 cannot be positioned within the evolution of data collection accuracy and validation.
- Foundational spatial and event-network methods are missing, including work on space control, pitch geometry, passing networks, and temporal movement modelling.
- Foundational expected-goals and shot-quality literature is missing, so the Anzer and Bauer entry cannot be evaluated as an extension of chance-quality and player-finishing work.
- Foundational player-action valuation studies predating VAEP, including earlier on-ball value and possession-value frameworks, are required to assess the novelty of Decroos and colleagues 2019.
- A systematic reproducibility and data-provenance source is missing for the public event-data entry; ownership, licensing, coverage, missing-event behavior, and identifiers need to be established.

## Actionable Corrections & Uncertainty Labels
- Goddard 2005: keep the title, DOI, and substantive record, but report 2004 as the DOI/publication-year metadata; if 2005 is retained as an issue or print year, explicitly label it as such.
- Hvattum and Arntzen 2010: report 2009 as the DOI/publication metadata unless a separately verified print-publication year is documented.
- Constantinou, Fenton, and Neil pi-football: remove the supplied DOI and 2011.10.003 association because the supplied cross-check resolves it to a different 2021 paper and DOI; verify the article record before retaining it.
- Koopman and Lit 2015: report 2013 as the DOI/publication metadata unless a verified 2015 print/issue year is established, and label online-versus-print dates explicitly.
- Groll, Schauberger, and Tutz: do not associate 10.3233/JSA-150006 with the listed paper until the DOI record is corrected; the supplied cross-check associates it with an Editorial.
- Bunker and Thabtah 2019: report 2017 as the DOI/publication-year metadata unless the journal's 2019 issue publication is independently verified.
- Mackenzie and Cushion 2013: report 2012 as the DOI/publication metadata unless 2013 is independently confirmed as the issue or print year.
- Lago-Peñas and colleagues 2010: retain the title and 2010 year only; remove or qualify 10.1080/0264041090358101 as a DOI until a publisher record verifies it because the cross-check found no DOI.
- Memmert, Lemmink, and Sampaio 2017: remove the supplied DOI and year because the cross-check points to a differently titled 2018 record with 10.4324/9781351210164; verify the exact intended journal article or chapter.
- Pappalardo and colleagues 2019: replace or verify 10.1038/sdata.2019.236 because the supplied cross-check reports 10.1038/s41597-019-0247-7 for the matching title and year.
- Anzer and Bauer 2021: remove 10.1371/journal.pone.0255518 because the supplied DOI resolves to an unrelated China CO2-flow paper; verify the PLOS ONE record and correct the identifier.
- Maher 1982, Dixon and Coles 1997, Rue and Salvesen 2000, Karlis and Ntzoufras 2003, Baio and Blangiardo 2010, Sarmento and colleagues 2014, Rein and Memmert 2016, Gudmundsson and Horton 2017, and Decroos and colleagues 2019 may be retained as verified title/year/DOI matches, but these checks do not verify substantive relevance or seminal status.
- Replace any recalled contribution, result, limitation, or seminal-status wording with NOT_VERIFIED until the relevant source text is added.
- Correct mismatched venue, year, title, and DOI fields to preserve separate DOI/publication and print/issue metadata instead of silently overwriting one with the other.

## Optional Deterministic Bibliographic Verification (OpenAlex)
- **Modelling association football scores** (https://doi.org/10.1111/j.1467-9574.1982.tb00782.x): `VERIFIED`
- **Modelling association football scores and inefficiencies in the football betting market** (https://doi.org/10.1111/1467-9876.00065): `VERIFIED`
- **Prediction and retrospective analysis of soccer matches in a league** (https://doi.org/10.1111/1467-9884.00243): `VERIFIED`
- **Analysis of sports data by using bivariate Poisson models** (https://doi.org/10.1111/1467-9884.00366): `VERIFIED`
- **Regression models for forecasting goals and match results in association football** (https://doi.org/10.1016/j.ijforecast.2004.08.002): `MISMATCH`
- **A Bayesian hierarchical model for the prediction of football results** (https://doi.org/10.1080/02664760802684177): `VERIFIED`
- **Using ELO ratings for match result prediction in association football** (https://doi.org/10.1016/j.ijforecast.2009.10.002): `MISMATCH`
- **pi-football: A Bayesian network model for forecasting association football match results** (https://doi.org/10.1016/j.ijforecast.2011.10.003): `MISMATCH`
- **A dynamic bivariate Poisson model for analysing and forecasting match results in the English Premier League** (https://doi.org/10.1111/rssa.12042): `MISMATCH`
- **Prediction of the FIFA World Cup 2014 – A random forest approach with an emphasis on chosen team composition** (https://doi.org/10.3233/JSA-150006): `MISMATCH`
- **A machine learning framework for sport result prediction** (https://doi.org/10.1016/j.aci.2017.09.005): `MISMATCH`
- **Performance analysis in football: A critical review and implications for future research** (https://doi.org/10.1080/02640414.2012.746720): `MISMATCH`
- **Match analysis in football: a systematic review** (https://doi.org/10.1080/02640414.2014.898852): `VERIFIED`
- **Game-related statistics that discriminated winning, drawing and losing teams from the Spanish soccer league** (https://doi.org/10.1080/0264041090358101): `MISMATCH`
- **Big data and tactical analysis in elite soccer: future challenges and opportunities for sports science** (https://doi.org/10.1186/s40064-016-3108-2): `VERIFIED`
- **Data analytics in football: positional data collection, modelling and analysis** (https://doi.org/10.1007/s40279-016-0664-6): `MISMATCH`
- **Spatio-temporal analysis of team sports** (https://doi.org/10.1145/3054132): `VERIFIED`
- **A public data set of spatio-temporal match events in soccer competitions** (https://doi.org/10.1038/sdata.2019.236): `MISMATCH`
- **Actions Speak Louder Than Goals: Valuing Player Actions in Soccer** (https://doi.org/10.1145/3292500.3330758): `VERIFIED`
- **A goal scoring probability model for shots based on players' abilities in soccer** (https://doi.org/10.1371/journal.pone.0255518): `MISMATCH`
