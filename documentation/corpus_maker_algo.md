There are five levels or gradients of segments sentences are constructed from.
There are two levels of sentences
The five phrasal or segment gradients are as follows:
1. AS - Advanced Spanish
2. MS - Moderate Spanish (previously called Simpler Spanish)
3. SS - Simple Spanish
4. ED - English Diglot
5. EN - English

The goal is to give the consumer of the generated final content comprehensible input while introducing new words a little at a time. Their comprehension of the vocabulary should be approximately at CT which defaults to 97%.

There are three categories of words with respect to comprehension.
1. Inactive: Spanish words not introduced yet
2. Active: Newly added words that are being learned and bring CT score down. Not considered comprehensible on their own, but in the context of surrounding comprehensible text.
3. Known: Either English words or Spanish words whose lemmas have been exposed to the learner a set number of times. This threshold is the ET or exposure threshold. The default for all words is 20 (although this may be an adjustable parameter in the system).

The lemmas with an ET that are an exception to the rule are shown below. The idea being that the 20 exposures should happen at different times spaced out throughout the text. Very common words would meet their ET within a very short period. The ET overrides in the chart below simulates seeing the word as if it were 1% of the time approximately even though in reality it is much higher.

+-----------+----------------------------+-------------------+
| Lemma     | Corpus Count (in Millions) | Required Exposures|
+-----------+----------------------------+-------------------+
| de        | 9.999518                   |               133 |
| la        | 6.277560                   |                84 |
| que       | 4.681839                   |                62 |
| el        | 4.569652                   |                61 |
| en        | 4.234281                   |                56 |
| y         | 4.180279                   |                56 |
| a         | 3.260939                   |                43 |
| los       | 2.618657                   |                35 |
| se        | 2.022514                   |                27 |
| del       | 1.857225                   |                25 |
| las       | 1.686741                   |                22 |
| un        | 1.659827                   |                22 |
| por       | 1.561904                   |                21 |
+-----------+----------------------------+-------------------+

The data for each sentence containing everything needed to construct all possible sentence variants with segments being expressed from either the AS and MS levels or the SS, ED and EN levels is called a deep sentence. There are many possible sentences that can be constructed from a deep sentence. 

The algo determines which phrases get expressed based on the known and active words. K/A. Sentences that contain inactive words should never be be expressed. Expressed sentences should contain segments as close to AS as possible. If a sentence built from only AS segments is possible without using inactive words, then it is the one that should be generated.

Along with the job of constructing sentences based on K/A, the job of the system is to trickle feed or 'activate' new words when a CT measurement is made resulting in a score above the CT number (default 97%).

There is a block size setting that is the number of sentences used for each of these calculations as well as the working set of sentences the system is generating at any one time. The default is 200 sentences.

The system after generating sentences of 200 or BS (block size) according to K/A will take a measurment of the block. It will count all Spanish lemmas used in the block both known and active as well as English words. All English is considered known.

Many books can be processed in a single run using a sequence.txt file containing the book names as a list. Books may be repeated in the run with subsequent books presumably containing more advanced Spanish.

Each book is divided into chunks of sentences of size BS with the last chunk being combined with the second to last if it is smaller than BS which is likely the case. A book of 430 sentences would result in two chunks if BS is set to 200 sentences. We want to subsume the last smaller chunk into it's predecessor to avoid more advanced Spanish being added to the learner's vocabulary prematurely.

Small sample sizes can lead to skewed CT measurements resulting in an artificially high number of new words being added or words that are more advanced than ideal. Larger block sizes gives a better statiscally meaningful set of new words to draw from when needed and a CT score that is more meaningful.

After a block of text is generated and the CT measurement is taken, if it is determined that new words need to be added, we want an efficient way to add words that would result in lower grades of Spanish being graduated from first. We want to graduate first from English phrases when they exist, then diglot, then Simple Spanish and so on.

We want a way to pinpoint exactly the sentences that would change by adding new vocabulary and selectively update those in order to take a new CT measurement. New cycles of adding new vocabulary and taking a CT measurement happen until the block or chunk of sentences pass after which the system moves onto the next chunk.

As each sentence is being processed for expression we can use a methodology to account for which lemmas would unlock or activate a segment if it were to be added. This allows for easily determining the optimal new vocabulary words when needed without re-visiting every sentence.

Additionally to aid in the efficiency of calculating the CT score, the system upon first loading a book will go through the data for each deep sentence and append an integer with every English segment called english_count. Later when the system calculates an expression sentence for that deep sentence, it will add up the expressed English sentence and subtract 1 for every diglot used and append an english_count value to the deep sentence data structure. In this way a CT calculation can be made efficiently by visiting each sentence in the block and using that number toward the known words count without having to re-count all the English words on every pass. The accounting for english_count for every sentence need be done only once per book processed.

