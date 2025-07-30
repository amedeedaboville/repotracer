I have an ambitious new feature I want to implement. I want to implement git-of-theseus (https://github.com/erikbern/git-of-theseus/blob/master/git_of_theseus/analyze.py) from scratch using gitoxide to make it super fast.

* Because we are doing "blame" on the entire repo at once, we can do things to speed it up (doing things in bulk)
* Also, because we only care about the year of each commit, and also have a resolution of one week, we can ignore lots of commits that blame would otherwise have to look at.
	* If a line gets added and then deleted within the week, it didn't exist for us. It's not within the resolution of the graph, it's just churn.
	* So we can look at which lines were introduced "which week", ie the diff of this file from last week instead of across every change. So only up to 52 "suspects" per year instead of for each commit
* Also, since we are plotting a file's content over time, we should be able to trace its entire history at once, instead of having to blame it every time it changes
	* So we start to from the beginning of a file, and we look at its state every week, and with that we can build a record for its entire history in one go instead of rebuilding it every commit
* We do care about renames, bc otherwise if there was a delete + a readd, the file contents would look like they got removed from their previous years and all their new contents would be reset to the current year. So that sucks

It looks like in libgit2 the relevant git function is git_diff_tree_to_tree, and in gix there's a  gix_diff::tree_with_rewrites function that compares two diffs.

I think the plan is:
* List all the last commits of the week on the main branch (there is existing code to this).
* Go through each commit with its previous week's one (for the first one we'll use an empty something)
	* we can do these pairs in parallel I believe
* For a pair of commits (one week apart), diff the two trees for those commits
	* This gives us a list of adds, deletes, renames and rewrites
* Adds get their entire content attributed to the new commit
* deletes remove their contents
* renames remove the historical content of the old file name and clone into the new file name
* and then rewrites:
	* For this we need to diff the two blobs
	* The older blob should have a table of Ranges that introduced it
	* the diff should tell us which ranges to modify
		* (start from the bottom) for each diffEntry/diffed Range (which I assume is a - and a + section), find the existing Ranges affected by the - section and remove that
		* maybe a function on a RangeEntryList -> deleteArea or something that appropriately shrinks or removes Ranges
		* and then for the + section, we need to insert a new range at the same place (which I guess fucks with the line numbers but we don't care)
		* I can see why they use linked lists here, we don't actually care about the line numbers, just splitting these things up
* in the end, we build a big list of HistogramDiffs, and then single threadedly (or in some parallel prefix scan) accumulate those diffs so that each point in time (weekly commit) has a HistogramEntry associated to it.
* Under the hood, each commit tree has full blame contents for each file in it, (with a bunch of them shared between trees)
* 
I think there should be a few new types, but the main data I see us holding is:

LineRange:
* stores eg [1,10) to be lines 1 through 9. 

FileBlameInfo:
 * a list of Ranges along with metadata about when they got introduced (eg which commit id and for now which year as well)

TreeBlameInfo:
 * an aggregation of FileBlameInfos, which sums up how many lines were introduced each year for all the files in this tree

BlameInfoDiff:
 * A list of ranges that get removed, and a list of ranges that get added (along with the BlameInfo metdata for these new additions)


